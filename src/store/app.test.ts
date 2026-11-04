import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  BackendError,
  setBackend,
  type AppStateInfo,
  type Backend,
  type BuildAck,
  type BuildEvent,
  type EntradaHistorial,
  type EstadoFiguras,
  type Limpieza,
  type Eol,
  type FileInfo,
  type InformeDoctor,
  type InformeReemplazo,
  type ProcesoTex,
  type Problema,
  type ResultadoBusqueda,
  type Simbolos,
  type SyncForward,
  type SyncInverse,
  type ProfileInfo,
  type ProjectInfo,
  type Propuesta,
  type StatePatch,
  type Tipico,
  type DisponibilidadTexpresso,
  type EventoTexpresso,
  type VistaTexpresso,
} from '../backend';
import { DEBOUNCE_CURSOR_MS, DEBOUNCE_TEXTO_MS, iniciarTexpresso, useTexpresso } from './texpresso';
import { cerrarTodos, editarDoc, getDoc, moverCursor, textoDe } from '../editor/docs';
import { BUILD_INICIAL, BUSQUEDA_INICIAL, FRAGMENTO_INICIAL, useApp } from './app';
import { PROYECTO_INICIAL, useProyecto } from './proyecto';
import { logBuffer } from './logbuffer';
import { pdfApi } from '../pdf/api';
import { FILTRO_INICIAL } from './problemas';

interface Archivo {
  text: string;
  version: string;
  eol: Eol;
  bom: boolean;
  mixed: boolean;
}

const perfil = (id: string, o: Partial<ProfileInfo> = {}): ProfileInfo => ({
  id,
  label: id,
  partial: false,
  externalize: false,
  parts: 'all',
  cleanBefore: false,
  autoCompile: true,
  hidden: false,
  draftImages: false,
  publica: false,
  partList: [],
  omit: [],
  color: null,
  vars: {},
  output: null,
  compileOn: null,
  ...o,
});

/** El estado de esta máquina de los tests: manual, para que guardar no dispare builds salvo en los tests que lo piden. */
const estadoBase = (): AppStateInfo => ({
  activeProfile: 'parcial',
  selection: ['Secciones/Uno'],
  variables: {},
  autoCompile: 'manual',
  idleMs: 500,
  outsideSave: 'warn',
  draftImages: false,
  currentPart: null,
});

function infoDe(o: Partial<ProjectInfo> = {}): ProjectInfo {
  return {
    name: 'p',
    root: '/p',
    candidates: ['main.tex'],
    rootFile: 'main.tex',
    configSaved: true,
    configError: null,
    config: {
      engine: 'pdflatex',
      profiles: [perfil('parcial', { partial: true, parts: 'selection' }), perfil('rapido', { partial: true }), perfil('completo', { autoCompile: false })],
      variables: [{ name: 'OpcionPie', label: 'Pie', choices: ['completo', 'autor'], default: 'completo' }],
      publishDir: '../PDF',
      exportDir: '../PDF/Capítulos',
    },
    state: estadoBase(),
    parts: [
      { name: 'Secciones/Uno', file: 'Secciones/Uno.tex', command: 'include' },
      { name: 'Secciones/Dos', file: 'Secciones/Dos.tex', command: 'include' },
      { name: 'Secciones/Tres', file: 'Secciones/Tres.tex', command: 'include' },
    ],
    indexMs: 3,
    lockedBy: null,
    piezas: [],
    ...o,
  };
}

/** Lo que detectaría el core de un proyecto como el libro, en chico (lo que la pantalla Proyecto muestra). */
function propuestaBase(raiz?: string): Propuesta {
  const config = { root: raiz ?? 'main.tex', engine: 'pdflatex', parts: { commands: ['include'] }, profiles: { completo: { label: 'Documento', autoCompile: false } } };
  return {
    raiz: raiz ?? 'main.tex',
    candidatas: [
      { archivo: 'main.tex', rol: 'principal', recomendada: true, motivo: 'El documento', alcanza: 9, includeonlyPropio: null, flagsPrevios: [], incluidaPor: [] },
      { archivo: 'cap_actual.tex', rol: 'auxiliar', recomendada: false, motivo: 'Raíz auxiliar', alcanza: 3, includeonlyPropio: 'Secciones/Uno', flagsPrevios: [], incluidaPor: [] },
    ],
    partes: ['Secciones/Uno', 'Secciones/Dos'],
    piezas: [],
    partsCommands: ['include'],
    flags: [],
    figuras: { modo: 'off', dir: 'figuras-cache', stampFiles: [], motivo: 'No usa TikZ', avisos: [] },
    perfiles: [{ id: 'completo', label: 'Documento', uso: 'El documento completo', tipico: null, publica: null }],
    salidas: { publicar: null, exportar: null, patron: '{num:02} - {name}.pdf', ejemploNombre: null },
    variables: [],
    avisos: [],
    texto: `${JSON.stringify(config, null, 2)}\n`,
  };
}

/** Un backend en memoria que registra qué se le pidió y en qué orden. */
function falso(archivos: Record<string, Partial<Archivo> & { text: string }>, proyecto?: Partial<ProjectInfo>) {
  const llamadas: string[] = [];
  const disco = new Map<string, Archivo>(
    Object.entries(archivos).map(([k, v]) => [k, { version: 'v0', eol: 'lf', bom: false, mixed: false, ...v }]),
  );
  let n = 1;
  const estado = { ...estadoBase(), ...proyecto?.state };
  const control = {
    demoraEscritura: null as Promise<void> | null,
    fileInfo: ((): FileInfo => ({ part: null, affects: 'yes', profileAuto: true, partsChanged: false })) as (rel: string) => FileInfo,
    ack: { kind: 'started', id: 1 } as BuildAck,
    problemas: [] as Problema[],
    forward: null as SyncForward | null,
    simbolos: { labels: [], refs: [], cites: [], bib: [], sections: [], defs: [], files: [], lint: [], scan_ms: 1 } as Simbolos,
    busqueda: { files: [], total: 0, truncated: false, files_scanned: 0, ms: 1 } as ResultadoBusqueda,
    buscar: null as null | (() => Promise<ResultadoBusqueda>),
    reemplazo: { changed: [], skipped: [], total: 0 } as InformeReemplazo,
    config: {} as Record<string, string>,
    ackBake: { kind: 'started', id: 1 } as BuildAck,
    figuras: { modo: 'project', dir: 'figuras-cache', sello: '6D06AB', actuales: 3, viejas: 0, sinSello: 0, bytes: 0, bytesViejos: 0, figlist: null, figuras: [], pendientes: 0, fallidas: 0, necesitaCocer: false, cocinando: null, errores: [] } as EstadoFiguras,
    limpieza: { archivos: [], bytes: 0, figuras: 0 } as Limpieza,
    errorLimpieza: null as string | null,
    inverse: null as SyncInverse | null,
    ackExport: { kind: 'started', id: 1 } as BuildAck,
    errorExport: null as string | null,
    /** Por qué se pidió cada build (el segundo argumento de `build`), sin ensuciar `llamadas`. */
    motivos: [] as (string | undefined)[],
    historial: [] as EntradaHistorial[],
    tipicos: {} as Record<string, Tipico>,
    /** La pantalla Proyecto: lo que detecta el core, el `latexviewer.json` del disco, si la config valida y lo que se escribió. */
    propuesta: ((raiz?: string): Propuesta => propuestaBase(raiz)) as (raiz?: string) => Propuesta,
    configEnDisco: null as string | null,
    validacionOk: true,
    escrito: [] as string[],
    informe: { hallazgos: [], ms: 5, distribucion: 'MiKTeX 26.5' } as InformeDoctor,
    huerfanos: [] as ProcesoTex[],
    /** Los PID que la interfaz mandó matar. */
    muertos: [] as number[],
    /** Lo que `onBuildEvent` y `onFsChanged` le entregaron a la interfaz: los tests los disparan a mano. */
    emitirBuild: (() => {}) as (e: BuildEvent) => void,
    emitirCambios: (() => {}) as (archivos: string[]) => void,
    /** TeXpresso: lo que contesta `texpressoActivar(true)` y lo que la interfaz recibe por `onTexpresso`. */
    texpresso: {
      vista: { fase: 'apagado', motivo: null, distro: null, reinicios: 0, arranqueMs: null, ultimoMs: null, errores: 0 } as VistaTexpresso,
      disponibilidad: { disponible: true, motivo: null, distro: 'Ubuntu', texpresso: null, completo: false } as DisponibilidadTexpresso,
      alPrender: { fase: 'iniciando', motivo: null, distro: 'Ubuntu', reinicios: 0, arranqueMs: null, ultimoMs: null, errores: 0 } as VistaTexpresso,
    },
    emitirTexpresso: (() => {}) as (e: EventoTexpresso) => void,
  };
  const vista = (o: Partial<ProjectInfo> = {}) => infoDe({ ...proyecto, state: { ...estado }, ...o });
  const b: Backend = {
    launchOptions: async () => ({ open: null, root: null, smoke: false, smokeMode: 'f1', smokeFile: null, smokePausaMs: 0 }),
    pickFolder: async () => '/p',
    openProject: async () => vista(),
    getProject: async () => {
      llamadas.push('getProject');
      return vista();
    },
    setRootFile: async (rel) => {
      llamadas.push(`raiz ${rel}`);
      return vista({ rootFile: rel });
    },
    setState: async (patch: StatePatch) => {
      llamadas.push(`estado ${JSON.stringify(patch)}`);
      Object.assign(estado, patch);
      return { ...estado };
    },
    saveConfig: async () => {
      llamadas.push('saveConfig');
      return vista({ configSaved: true });
    },
    fileInfo: async (rel) => {
      llamadas.push(`info ${rel}`);
      return control.fileInfo(rel);
    },
    listDir: async () => [],
    readFile: async (rel) => {
      llamadas.push(`read ${rel}`);
      const f = disco.get(rel);
      if (!f) throw new BackendError('not_found', rel);
      if (rel.endsWith('.bin')) throw new BackendError('not_text', 'binario');
      return { ...f };
    },
    writeFile: async (rel, text, esperada) => {
      llamadas.push(`write ${rel}`);
      if (control.demoraEscritura) await control.demoraEscritura;
      const f = disco.get(rel)!;
      if (esperada !== f.version) throw new BackendError('conflict', 'cambió', f.version);
      if (f.text === text) return { written: false, version: f.version };
      f.text = text;
      f.version = `v${n++}`;
      return { written: true, version: f.version };
    },
    readPdf: async () => new Uint8Array(),
    build: async (p, motivo) => {
      llamadas.push(p ? `build ${p}` : 'build');
      control.motivos.push(motivo);
      return control.ack;
    },
    history: async () => control.historial,
    typicalTimes: async () => control.tipicos,
    detectProject: async (raiz) => control.propuesta(raiz),
    configOnDisk: async () => control.configEnDisco,
    validateConfig: async (texto) => ({ ok: control.validacionOk, errores: control.validacionOk ? [] : ['mal'], normalizado: control.validacionOk ? texto : null, ejemploNombre: null }),
    writeConfig: async (texto) => {
      control.escrito.push(texto);
      return vista({ configSaved: true });
    },
    probeConfig: async () => ({ doctor: control.informe, listado: { parte: null, figuras: 0, ms: 1, nombres: [], omitido: 'sin cache' } }),
    runDoctor: async () => control.informe,
    orphans: async () => control.huerfanos,
    killOrphans: async (pids) => {
      control.muertos.push(...pids);
      return pids;
    },
    cancelBuild: async () => {
      llamadas.push('cancel');
      return true;
    },
    problems: async (p) => {
      llamadas.push(`problems ${p}`);
      return control.problemas;
    },
    synctexForward: async (rel, line) => {
      llamadas.push(`forward ${rel}:${line}`);
      return control.forward;
    },
    getSymbols: async () => control.simbolos,
    figuresStatus: async () => {
      llamadas.push('figStatus');
      return control.figuras;
    },
    figuresBake: async () => {
      llamadas.push('figBake');
      return control.ackBake;
    },
    figuresCleanPreview: async () => {
      llamadas.push('figVista');
      if (control.errorLimpieza) throw new BackendError('other', control.errorLimpieza);
      return control.limpieza;
    },
    figuresClean: async () => {
      llamadas.push('figLimpiar');
      return control.limpieza;
    },
    searchProject: async (o, r) => {
      llamadas.push(`buscar ${o.query}${r !== undefined ? ` -> ${r}` : ''}`);
      return control.buscar ? control.buscar() : control.busqueda;
    },
    replaceProject: async (o, r, files) => {
      llamadas.push(`reemplazar ${o.query} -> ${r} en ${files ? files.join(',') : 'todos'}`);
      return control.reemplazo;
    },
    readUserConfig: async (n) => control.config[n] ?? null,
    writeUserConfig: async (n, t) => {
      control.config[n] = t;
      return `C:/cfg/${n}`;
    },
    synctexInverse: async (perfil, page, x, y) => {
      llamadas.push(`inverse ${perfil} ${page} ${Math.round(x)},${Math.round(y)}`);
      return control.inverse;
    },
    publish: async (p) => {
      llamadas.push(`publish ${p}`);
      return { pdf: `C:/pub/${p}.pdf`, synctex: null };
    },
    openOutputDir: async (cual) => {
      llamadas.push(`carpeta ${cual}`);
      return `C:/pub/${cual}`;
    },
    exportParts: async (partes) => {
      llamadas.push(`exportar ${partes ? partes.join(',') : 'todas'}`);
      if (control.errorExport) throw new BackendError('other', control.errorExport);
      return control.ackExport;
    },
    previewFragment: async (text) => {
      llamadas.push(`fragmento ${JSON.stringify(text)}`);
      return { kind: 'started', id: 1 };
    },
    onBuildEvent: async (cb) => {
      control.emitirBuild = cb;
      return () => {};
    },
    onFsChanged: async (cb) => {
      control.emitirCambios = cb;
      return () => {};
    },
    onCloseRequested: async () => () => {},
    exitApp: async () => {},
    smokeReadBytes: async () => new Uint8Array(),
    smokeMtime: async () => 0,
    writeSmokeResult: async () => {},
    texpressoEstado: async () => ({ vista: control.texpresso.vista, disponibilidad: control.texpresso.disponibilidad }),
    texpressoActivar: async (on) => {
      llamadas.push(`tp activar ${on}`);
      control.texpresso.vista = on ? control.texpresso.alPrender : { ...control.texpresso.vista, fase: 'apagado' };
      return { vista: control.texpresso.vista, disponibilidad: control.texpresso.disponibilidad };
    },
    texpressoBuffer: async (rel, text) => void llamadas.push(`tp buffer ${rel} ${JSON.stringify(text)}`),
    texpressoCerrar: async (rel) => void llamadas.push(`tp cerrar ${rel}`),
    texpressoPosicion: async (rel, line) => void llamadas.push(`tp posicion ${rel}:${line}`),
    texpressoVivo: async (ancho) => void llamadas.push(`tp vivo ${ancho}`),
    texpressoVivoClic: async (x, y) => void llamadas.push(`tp vivo-clic ${x},${y}`),
    texpressoPaginaVivo: async (d) => void llamadas.push(`tp pagina ${d}`),
    onTexpresso: async (cb) => {
      control.emitirTexpresso = cb;
      return () => {};
    },
  };
  setBackend(b);
  return { llamadas, disco, control, estado };
}

const st = () => useApp.getState();
const tab = (rel: string) => st().tabs.find((t) => t.rel === rel)!;

async function responder(id: string) {
  await vi.waitFor(() => expect(st().dialog).not.toBeNull());
  st().responderDialogo(id);
}

beforeEach(() => {
  cerrarTodos();
  logBuffer.clear();
  useProyecto.setState({ ...PROYECTO_INICIAL });
  useApp.setState({
    project: infoDe(),
    rootFile: 'main.tex',
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
    notice: null,
    dialog: null,
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe('abrir y editar', () => {
  it('abrir crea una pestaña limpia y activa, y reabrir no vuelve a leer', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n' } });
    await st().abrirArchivo('a.tex');
    expect(st().activeRel).toBe('a.tex');
    expect(tab('a.tex')).toMatchObject({ name: 'a.tex', dirty: false, readOnly: false });
    await st().abrirArchivo('a.tex');
    expect(f.llamadas.filter((l) => l === 'read a.tex')).toHaveLength(1);
    expect(st().tabs).toHaveLength(1);
  });

  it('la pestaña muestra el nombre sin la carpeta', async () => {
    falso({ 'Secciones/Capítulo uno.tex': { text: 'x' } });
    await st().abrirArchivo('Secciones/Capítulo uno.tex');
    expect(tab('Secciones/Capítulo uno.tex').name).toBe('Capítulo uno.tex');
  });

  it('un archivo que no es texto avisa y no abre pestaña', async () => {
    falso({ 'x.bin': { text: '' } });
    await st().abrirArchivo('x.bin');
    expect(st().tabs).toHaveLength(0);
    expect(st().notice?.nivel).toBe('error');
  });

  it('una imagen o un PDF abren una pestaña de visor sin leerse como texto', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n' } });
    await st().abrirArchivo('Circuitos/TC ST.png');
    await st().abrirArchivo('Circuitos/Pierce.PDF');
    expect(f.llamadas.filter((l) => l.startsWith('read '))).toHaveLength(0);
    expect(tab('Circuitos/TC ST.png')).toMatchObject({ name: 'TC ST.png', visor: 'imagen', readOnly: true, dirty: false });
    expect(tab('Circuitos/Pierce.PDF').visor).toBe('pdf');
    expect(st().activeRel).toBe('Circuitos/Pierce.PDF');
    // Reabrir la activa, pasar a otra y volver: siempre la misma pestaña.
    await st().abrirArchivo('a.tex');
    st().activar('Circuitos/TC ST.png');
    expect(st().activeRel).toBe('Circuitos/TC ST.png');
    await st().abrirArchivo('Circuitos/TC ST.png');
    expect(st().tabs).toHaveLength(3);
    await st().cerrarTab('Circuitos/TC ST.png');
    expect(st().tabs.map((t) => t.rel)).toEqual(['Circuitos/Pierce.PDF', 'a.tex']);
  });

  it('una imagen abierta que cambia afuera sube su epoch (el visor la vuelve a leer)', async () => {
    falso({});
    await st().abrirArchivo('fig.png');
    expect(tab('fig.png').epoch).toBe(0);
    await st().alCambiarAfuera(['fig.png']);
    expect(tab('fig.png').epoch).toBe(1);
  });

  it('editar marca modificado; deshacer el cambio lo deja limpio', async () => {
    falso({ 'a.tex': { text: 'uno\ndos\n' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    expect(tab('a.tex').dirty).toBe(true);
    editarDoc('a.tex', { from: 0, to: 1 });
    expect(tab('a.tex').dirty).toBe(false);
  });
});

describe('guardar', () => {
  it('escribe con la versión leída, deja la pestaña limpia y el segundo guardado no llama al backend', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n', version: 'v0' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    expect(await st().guardar('a.tex')).toBe('guardado');
    expect(f.disco.get('a.tex')!.text).toBe('Xuno\n');
    expect(tab('a.tex').dirty).toBe(false);
    expect(getDoc('a.tex')!.version).toBe('v1');
    expect(await st().guardar('a.tex')).toBe('sin-cambios');
    expect(f.llamadas.filter((l) => l === 'write a.tex')).toHaveLength(1);
    // Forzado (lo usa el smoke): llega al core, que dice que no escribió nada.
    expect(await st().guardar('a.tex', { forzar: true })).toBe('sin-cambios');
    expect(f.llamadas.filter((l) => l === 'write a.tex')).toHaveLength(2);
  });

  it('si el usuario sigue escribiendo mientras se guarda, la pestaña sigue modificada', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n' } });
    let soltar!: () => void;
    f.control.demoraEscritura = new Promise<void>((r) => (soltar = r));
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'A' });
    const guardando = st().guardar('a.tex');
    editarDoc('a.tex', { from: 1, insert: 'B' }); // mientras tanto
    soltar();
    expect(await guardando).toBe('guardado');
    expect(f.disco.get('a.tex')!.text).toBe('Auno\n'); // lo que se escribió es lo de antes del await
    expect(tab('a.tex').dirty).toBe(true); // y lo nuevo sigue sin guardar
  });

  it('conflicto de versión: pregunta y, si se acepta, reintenta con la versión del disco', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n', version: 'v0' } });
    await st().abrirArchivo('a.tex');
    f.disco.get('a.tex')!.version = 'v9'; // alguien lo tocó afuera
    f.disco.get('a.tex')!.text = 'cambiado afuera\n';
    editarDoc('a.tex', { from: 0, insert: 'X' });
    const g = st().guardar('a.tex');
    await responder('sobrescribir');
    expect(await g).toBe('guardado');
    expect(f.disco.get('a.tex')!.text).toBe('Xuno\n');
    expect(tab('a.tex').dirty).toBe(false);
  });

  it('conflicto de versión: si se cancela, no se pisa y la pestaña sigue modificada', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n', version: 'v0' } });
    await st().abrirArchivo('a.tex');
    f.disco.get('a.tex')!.version = 'v9';
    f.disco.get('a.tex')!.text = 'cambiado afuera\n';
    editarDoc('a.tex', { from: 0, insert: 'X' });
    const g = st().guardar('a.tex');
    await responder('cancelar');
    expect(await g).toBe('cancelado');
    expect(f.disco.get('a.tex')!.text).toBe('cambiado afuera\n');
    expect(tab('a.tex').dirty).toBe(true);
  });

  it('un archivo con EOL mezclados es de sólo lectura hasta que se acepta normalizar', async () => {
    const f = falso({ 'm.tex': { text: 'uno\ndos\n', mixed: true, eol: 'crlf' } });
    await st().abrirArchivo('m.tex');
    expect(tab('m.tex')).toMatchObject({ readOnly: true, mixed: true });
    expect(st().notice?.texto).toContain('sólo lectura');
    expect(await st().guardar('m.tex')).toBe('sin-cambios');
    expect(f.llamadas).not.toContain('write m.tex');
    const epoch = tab('m.tex').epoch;
    st().hacerEditable('m.tex');
    expect(tab('m.tex')).toMatchObject({ readOnly: false });
    expect(tab('m.tex').epoch).toBe(epoch + 1);
    editarDoc('m.tex', { from: 0, insert: 'X' });
    expect(await st().guardar('m.tex')).toBe('guardado');
  });
});

describe('cerrar pestañas', () => {
  it('con cambios pregunta; "no guardar" cierra sin escribir y pasa a la vecina', async () => {
    const f = falso({ 'a.tex': { text: 'a' }, 'b.tex': { text: 'b' }, 'c.tex': { text: 'c' } });
    for (const r of ['a.tex', 'b.tex', 'c.tex']) await st().abrirArchivo(r);
    st().activar('b.tex');
    editarDoc('b.tex', { from: 0, insert: 'X' });
    const cerrando = st().cerrarTab('b.tex');
    await responder('descartar');
    await cerrando;
    expect(st().tabs.map((t) => t.rel)).toEqual(['a.tex', 'c.tex']);
    expect(st().activeRel).toBe('c.tex');
    expect(f.llamadas).not.toContain('write b.tex');
  });

  it('cancelar la pregunta deja todo como estaba; guardar escribe y cierra', async () => {
    const f = falso({ 'a.tex': { text: 'a' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    const c1 = st().cerrarTab('a.tex');
    await responder('cancelar');
    await c1;
    expect(st().tabs).toHaveLength(1);
    const c2 = st().cerrarTab('a.tex');
    await responder('guardar');
    await c2;
    expect(st().tabs).toHaveLength(0);
    expect(st().activeRel).toBeNull();
    expect(f.disco.get('a.tex')!.text).toBe('Xa');
  });
});

describe('compilar', () => {
  it('guarda todo ANTES de lanzar el build', async () => {
    const f = falso({ 'a.tex': { text: 'a' }, 'b.tex': { text: 'b' } });
    await st().abrirArchivo('a.tex');
    await st().abrirArchivo('b.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    editarDoc('b.tex', { from: 0, insert: 'Y' });
    await st().compilar();
    expect(f.llamadas.filter((l) => /^(write|build)/.test(l))).toEqual(['write a.tex', 'write b.tex', 'build']);
    expect(st().build.status).toBe('running');
  });

  it('con la auto-compilación al guardar, compilar no pide dos builds (guarda sin disparar la suya)', async () => {
    const f = falso({ 'a.tex': { text: 'a' } }, { state: { ...estadoBase(), autoCompile: 'save' } });
    useApp.setState({ project: infoDe({ state: { ...estadoBase(), autoCompile: 'save' } }) });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    await st().compilar();
    await new Promise((r) => setTimeout(r, 60));
    expect(f.llamadas.filter((l) => l.startsWith('build'))).toHaveLength(1);
  });

  it('sin raíz o sin proyecto no compila y avisa', async () => {
    const f = falso({});
    useApp.setState({ rootFile: null });
    await st().compilar();
    expect(f.llamadas).not.toContain('build');
    expect(st().notice?.texto).toContain('raíz');
  });

  it('si guardar se cancela, no compila', async () => {
    const f = falso({ 'a.tex': { text: 'a', version: 'v0' } });
    await st().abrirArchivo('a.tex');
    f.disco.get('a.tex')!.version = 'v9';
    editarDoc('a.tex', { from: 0, insert: 'X' });
    const c = st().compilar();
    await responder('cancelar');
    await c;
    expect(f.llamadas).not.toContain('build');
    expect(st().build.status).toBe('idle');
  });

  it('si el backend rechaza el build, la barra vuelve a idle y se avisa', async () => {
    const f = falso({});
    const b = (await import('../backend')).getBackend();
    b.build = async () => {
      throw new BackendError('other', 'elegí al menos una parte para compilar');
    };
    await st().compilar();
    expect(f.llamadas).not.toContain('build');
    expect(st().build.status).toBe('idle');
    expect(st().notice?.nivel).toBe('error');
  });

  it('un pedido mientras corre otro queda pendiente y no cambia el estado de la barra', async () => {
    const f = falso({});
    f.control.ack = { kind: 'queued' };
    st().manejarEventoBuild({ type: 'started', id: 1, perfil: 'parcial', cmdline: 'latexmk', retry: false });
    await st().compilar('rapido');
    expect(f.llamadas).toContain('build rapido');
    expect(st().build).toMatchObject({ status: 'running', pendiente: 'rapido' });
  });

  it('cancelar descarta el pendiente en la barra y le avisa al core', async () => {
    const f = falso({});
    st().manejarEventoBuild({ type: 'started', id: 1, perfil: 'parcial', cmdline: 'latexmk', retry: false });
    st().manejarEventoBuild({ type: 'queued', perfil: 'rapido' });
    expect(st().build.pendiente).toBe('rapido');
    await st().cancelarBuild();
    expect(st().build.pendiente).toBeNull();
    expect(f.llamadas).toContain('cancel');
  });
});

describe('eventos del build', () => {
  const inicio = (id = 1, retry = false): BuildEvent => ({ type: 'started', id, perfil: 'completo', cmdline: 'latexmk -pdf x', retry });
  const fin = (o: Partial<Extract<BuildEvent, { type: 'finished' }>> = {}): BuildEvent => ({
    type: 'finished',
    id: 1,
    perfil: 'completo',
    ok: true,
    code: 0,
    cancelled: false,
    ms: 1500,
    pdf: '.lv/build/completo/lv-completo.pdf',
    updated: true,
    passes: 2,
    truncated_aux: false,
    repaired: false,
    errors: 0,
    warnings: 0,
    badboxes: 0,
    ...o,
  });

  it('started limpia el log y pone la línea de comandos; log agrega texto', () => {
    logBuffer.append('de antes\n');
    st().manejarEventoBuild(inicio());
    st().manejarEventoBuild({ type: 'log', id: 1, text: 'hola\n[1] ' });
    st().manejarEventoBuild({ type: 'log', id: 1, text: '[2]' });
    expect(logBuffer.slice(0, logBuffer.length)).toEqual(['$ latexmk -pdf x', 'hola', '[1] [2]']);
    expect(st().build).toMatchObject({ status: 'running', id: 1, perfil: 'completo' });
  });

  it('el reintento después de reparar un .aux sigue en el mismo log', () => {
    st().manejarEventoBuild(inicio());
    st().manejarEventoBuild({ type: 'log', id: 1, text: '! File ended while scanning use of \\@newl@bel.\n' });
    st().manejarEventoBuild(inicio(2, true));
    const log = logBuffer.slice(0, logBuffer.length).join('\n');
    expect(log).toContain('File ended while scanning');
    expect(log.match(/\$ latexmk -pdf x/g)).toHaveLength(2);
    expect(st().build.repaired).toBe(true);
    st().manejarEventoBuild(fin({ repaired: true }));
    expect(logBuffer.slice(0, logBuffer.length).join('\n')).toContain('después de reparar los auxiliares');
  });

  it('un build bueno con PDF actualizado recarga el visor y muestra las pasadas', () => {
    st().manejarEventoBuild(inicio());
    st().manejarEventoBuild(fin());
    expect(st().build).toMatchObject({ status: 'ok', ms: 1500, code: 0, passes: 2 });
    expect(st().pdfRel).toBe('.lv/build/completo/lv-completo.pdf');
    expect(st().pdfEpoch).toBe(1);
    expect(st().pdfStale).toBe(false);
    expect(logBuffer.slice(0, logBuffer.length).join('\n')).toContain('terminó bien en 1.5 s, 2 pasadas');
  });

  it('un build sin cambios no recarga el visor si ya muestra ese PDF, pero sí si todavía no hay ninguno', () => {
    st().manejarEventoBuild(fin({ updated: false })); // todavía no había PDF en el visor
    expect(st().pdfEpoch).toBe(1);
    st().manejarEventoBuild(fin({ updated: false }));
    expect(st().pdfEpoch).toBe(1);
    st().manejarEventoBuild(fin({ updated: true }));
    expect(st().pdfEpoch).toBe(2);
  });

  it('un build que falla no toca el visor; si hay un PDF, queda marcado como desactualizado hasta el próximo bueno', () => {
    st().manejarEventoBuild(fin({ ok: false, code: 12 }));
    expect(st().build).toMatchObject({ status: 'fail', code: 12 });
    expect(st().pdfRel).toBeNull();
    expect(st().pdfEpoch).toBe(0);
    expect(st().pdfStale).toBe(false); // no hay PDF que marcar
    expect(logBuffer.slice(0, logBuffer.length).join('\n')).toContain('falló (código 12)');
    st().manejarEventoBuild(fin());
    expect(st().pdfStale).toBe(false);
    st().manejarEventoBuild(fin({ ok: false, code: 12 }));
    expect(st().pdfRel).toBe('.lv/build/completo/lv-completo.pdf');
    expect(st().pdfEpoch).toBe(1);
    expect(st().pdfStale).toBe(true);
    st().manejarEventoBuild(fin({ updated: true }));
    expect(st().pdfStale).toBe(false);
    expect(st().pdfEpoch).toBe(2);
  });

  it('cancelado queda como cancelado y también marca el PDF como desactualizado', () => {
    st().manejarEventoBuild(fin());
    st().manejarEventoBuild(fin({ ok: false, cancelled: true, code: 1, pdf: null }));
    expect(st().build.status).toBe('cancelled');
    expect(st().pdfStale).toBe(true);
  });

  it('queued marca el pendiente, started lo limpia y un error del core se avisa', () => {
    st().manejarEventoBuild(inicio());
    st().manejarEventoBuild({ type: 'queued', perfil: 'rapido' });
    expect(st().build.pendiente).toBe('rapido');
    st().manejarEventoBuild(fin());
    expect(st().build.pendiente).toBe('rapido'); // sigue hasta que arranque el pendiente
    st().manejarEventoBuild(inicio(2));
    expect(st().build.pendiente).toBeNull();
    st().manejarEventoBuild({ type: 'queued', perfil: 'rapido' });
    st().manejarEventoBuild({ type: 'error', message: 'no se pudo lanzar el build pendiente' });
    expect(st().build.pendiente).toBeNull();
    expect(st().notice).toMatchObject({ nivel: 'error', texto: 'no se pudo lanzar el build pendiente' });
  });
});

describe('perfiles y partes', () => {
  it('elegir perfil y variable van al core y se reflejan', async () => {
    const f = falso({});
    await st().elegirPerfil('rapido');
    expect(st().project!.state.activeProfile).toBe('rapido');
    await st().setVariable('OpcionPie', 'autor');
    expect(st().project!.state.variables).toEqual({ OpcionPie: 'autor' });
    expect(f.llamadas.filter((l) => l.startsWith('estado'))).toHaveLength(2);
  });

  it('alternar parte la agrega en el orden del documento, no deja la selección vacía y "todas" las tilda', async () => {
    falso({});
    await st().alternarParte('Secciones/Tres');
    await st().alternarParte('Secciones/Dos');
    expect(st().project!.state.selection).toEqual(['Secciones/Uno', 'Secciones/Dos', 'Secciones/Tres']);
    await st().alternarParte('Secciones/Uno');
    await st().alternarParte('Secciones/Dos');
    expect(st().project!.state.selection).toEqual(['Secciones/Tres']);
    await st().alternarParte('Secciones/Tres'); // la última no se destilda
    expect(st().project!.state.selection).toEqual(['Secciones/Tres']);
    await st().soloParte('Secciones/Dos');
    expect(st().project!.state.selection).toEqual(['Secciones/Dos']);
    await st().todasLasPartes();
    expect(st().project!.state.selection).toHaveLength(3);
  });

  it('un error del core al cambiar el estado se avisa y no cambia nada', async () => {
    falso({});
    const b = (await import('../backend')).getBackend();
    b.setState = async () => {
      throw new BackendError('other', 'no existe el perfil "x"');
    };
    await st().elegirPerfil('x');
    expect(st().project!.state.activeProfile).toBe('parcial');
    expect(st().notice?.nivel).toBe('error');
  });

  it('compilar una parte deja sólo esa parte, usa un perfil de partes elegidas y compila sin volver a guardar', async () => {
    const f = falso({ 'a.tex': { text: 'a' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    st().project!.state.activeProfile = 'completo';
    await st().compilarParte('Secciones/Dos', { guardar: false });
    expect(f.estado.selection).toEqual(['Secciones/Dos']);
    expect(f.estado.activeProfile).toBe('parcial');
    expect(f.llamadas).toContain('build parcial');
    expect(f.llamadas).not.toContain('write a.tex');
  });

  it('compilar la parte del archivo abierto usa lo que dice el core; sin parte, avisa', async () => {
    const f = falso({ 'Secciones/Dos.tex': { text: 'dos' }, 'suelto.tex': { text: 'x' } });
    f.control.fileInfo = (rel) => ({ part: rel.startsWith('Secciones/') ? 'Secciones/Dos' : null, affects: 'yes', profileAuto: true, partsChanged: false });
    await st().abrirArchivo('suelto.tex');
    await st().compilarParteActiva();
    expect(st().notice?.texto).toContain('no pertenece a ninguna parte');
    expect(f.llamadas.filter((l) => l.startsWith('build'))).toHaveLength(0);
    await st().abrirArchivo('Secciones/Dos.tex');
    await st().compilarParteActiva();
    expect(f.estado.selection).toEqual(['Secciones/Dos']);
    expect(f.llamadas).toContain('build parcial');
  });

  it('la parte del archivo activo se actualiza al cambiar de pestaña', async () => {
    const f = falso({ 'Secciones/Uno.tex': { text: 'u' }, 'main.tex': { text: 'm' } });
    f.control.fileInfo = (rel) => ({ part: rel.startsWith('Secciones/') ? 'Secciones/Uno' : null, affects: 'yes', profileAuto: true, partsChanged: false });
    await st().abrirArchivo('Secciones/Uno.tex');
    await vi.waitFor(() => expect(st().parteActiva).toBe('Secciones/Uno'));
    await st().abrirArchivo('main.tex');
    await vi.waitFor(() => expect(st().parteActiva).toBeNull());
    st().activar('Secciones/Uno.tex');
    await vi.waitFor(() => expect(st().parteActiva).toBe('Secciones/Uno'));
  });

  it('publicar usa el perfil del último build y avisa dónde quedó; guardar la configuración la deja como guardada', async () => {
    const f = falso({});
    st().manejarEventoBuild({ type: 'started', id: 1, perfil: 'rapido', cmdline: 'x', retry: false });
    await st().publicar();
    expect(f.llamadas).toContain('publish rapido');
    expect(st().notice?.texto).toBe('Publicado: C:/pub/rapido.pdf');
    useApp.setState({ project: infoDe({ configSaved: false }) });
    await st().guardarConfig();
    expect(f.llamadas).toContain('saveConfig');
    expect(st().project!.configSaved).toBe(true);
  });
});

describe('auto-compilación', () => {
  const conModo = (modo: AppStateInfo['autoCompile'], extra: Partial<AppStateInfo> = {}) => {
    const state = { ...estadoBase(), autoCompile: modo, ...extra };
    useApp.setState({ project: infoDe({ state }) });
    return state;
  };
  const con = (afecta: Partial<FileInfo>) => {
    const f = falso({ 'a.tex': { text: 'a' } });
    f.control.fileInfo = () => ({ part: null, affects: 'yes', profileAuto: true, partsChanged: false, ...afecta });
    return f;
  };
  const compilaron = (f: ReturnType<typeof falso>) => f.llamadas.filter((l) => l.startsWith('build'));

  it('Ctrl+S con el modo "al guardar" compila el perfil activo si el archivo lo afecta', async () => {
    conModo('save');
    const f = con({ affects: 'yes' });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    await st().guardar('a.tex');
    await vi.waitFor(() => expect(compilaron(f)).toEqual(['build']));
    expect(f.llamadas).toContain('info a.tex');
    expect(f.llamadas).not.toContain('getProject');
  });

  it('en modo manual, o si el perfil no compila al guardar, o si el archivo no está en el grafo, no compila', async () => {
    for (const caso of [
      { modo: 'manual' as const, info: { affects: 'yes' as const } },
      { modo: 'save' as const, info: { affects: 'yes' as const, profileAuto: false } },
      { modo: 'save' as const, info: { affects: 'unrelated' as const } },
    ]) {
      conModo(caso.modo);
      const f = con(caso.info);
      cerrarTodos();
      useApp.setState({ tabs: [], activeRel: null });
      await st().abrirArchivo('a.tex');
      editarDoc('a.tex', { from: 0, insert: 'X' });
      await st().guardar('a.tex');
      await vi.waitFor(() => expect(f.llamadas).toContain('info a.tex'));
      await new Promise((r) => setTimeout(r, 30));
      expect(compilaron(f), JSON.stringify(caso)).toHaveLength(0);
    }
  });

  it('guardar sin cambios no consulta ni compila', async () => {
    conModo('save');
    const f = con({});
    await st().abrirArchivo('a.tex');
    await new Promise((r) => setTimeout(r, 10));
    f.llamadas.length = 0; // abrir ya pregunta por la parte del archivo; lo que importa es lo que viene después
    expect(await st().guardar('a.tex', { forzar: true })).toBe('sin-cambios');
    await new Promise((r) => setTimeout(r, 30));
    expect(f.llamadas).not.toContain('info a.tex');
    expect(compilaron(f)).toHaveLength(0);
  });

  it('una parte nueva en el proyecto vuelve a leerlo', async () => {
    conModo('manual');
    const f = con({ partsChanged: true });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    await st().guardar('a.tex');
    await vi.waitFor(() => expect(f.llamadas).toContain('getProject'));
  });

  describe('archivo de una parte que el perfil activo no incluye', () => {
    const preparar = (outside: AppStateInfo['outsideSave']) => {
      conModo('save', { outsideSave: outside });
      return con({ affects: 'other_part', part: 'Secciones/Dos' });
    };
    const guardarA = async () => {
      await st().abrirArchivo('a.tex');
      editarDoc('a.tex', { from: 0, insert: 'X' });
      await st().guardar('a.tex');
    };

    it('avisar: ofrece "Compilar esta parte" y no compila hasta que se acepta', async () => {
      const f = preparar('warn');
      await guardarA();
      await vi.waitFor(() => expect(st().notice?.accion?.label).toBe('Compilar esta parte'));
      expect(st().notice?.texto).toContain('Dos');
      expect(compilaron(f)).toHaveLength(0);
      st().notice!.accion!.run();
      await vi.waitFor(() => expect(compilaron(f)).toEqual(['build parcial']));
      expect(f.estado.selection).toEqual(['Secciones/Dos']);
    });

    it('cambiar la selección: selecciona esa parte y compila', async () => {
      const f = preparar('switch');
      await guardarA();
      await vi.waitFor(() => expect(compilaron(f)).toEqual(['build parcial']));
      expect(f.estado.selection).toEqual(['Secciones/Dos']);
    });

    it('compilar igual: compila el perfil activo sin tocar la selección', async () => {
      const f = preparar('build');
      await guardarA();
      await vi.waitFor(() => expect(compilaron(f)).toEqual(['build']));
      expect(f.estado.selection).toEqual(['Secciones/Uno']);
    });
  });

  it('al dejar de tipear: guarda una vez pasado el tiempo, y cada tecla reinicia la cuenta', async () => {
    vi.useFakeTimers();
    conModo('idle', { idleMs: 500 });
    const f = con({ affects: 'yes' });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    st().alEditar('a.tex');
    await vi.advanceTimersByTimeAsync(400);
    editarDoc('a.tex', { from: 1, insert: 'Y' });
    st().alEditar('a.tex');
    await vi.advanceTimersByTimeAsync(400);
    expect(f.llamadas).not.toContain('write a.tex');
    await vi.advanceTimersByTimeAsync(200);
    expect(f.disco.get('a.tex')!.text).toBe('XYa');
    await vi.advanceTimersByTimeAsync(10);
    expect(compilaron(f)).toEqual(['build']);
  });

  it('el modo "al dejar de tipear" no hace nada si el modo es otro, y cambiar de modo cancela lo pendiente', async () => {
    vi.useFakeTimers();
    conModo('idle', { idleMs: 500 });
    const f = con({ affects: 'yes' });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    st().alEditar('a.tex');
    await st().setAutoCompile('manual');
    await vi.advanceTimersByTimeAsync(2000);
    expect(f.llamadas).not.toContain('write a.tex');
    conModo('save');
    st().alEditar('a.tex');
    await vi.advanceTimersByTimeAsync(2000);
    expect(f.llamadas).not.toContain('write a.tex');
  });
});

describe('cierre de la ventana y cambio de proyecto', () => {
  it('sin cambios se puede cerrar sin preguntar', async () => {
    falso({ 'a.tex': { text: 'a' } });
    await st().abrirArchivo('a.tex');
    expect(await st().permitirCierre()).toBe(true);
    expect(st().dialog).toBeNull();
  });

  it('con cambios: descartar cierra, cancelar no, guardar escribe y cierra', async () => {
    const f = falso({ 'a.tex': { text: 'a' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    let p = st().permitirCierre();
    await responder('cancelar');
    expect(await p).toBe(false);
    p = st().permitirCierre();
    await responder('descartar');
    expect(await p).toBe(true);
    expect(f.llamadas).not.toContain('write a.tex');
    p = st().permitirCierre();
    await responder('guardar');
    expect(await p).toBe(true);
    expect(f.disco.get('a.tex')!.text).toBe('Xa');
  });

  it('abrir otro proyecto con cambios sin guardar pregunta antes y cerrarlo todo', async () => {
    falso({ 'a.tex': { text: 'a' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    const p = st().abrirProyecto('/otro');
    await responder('cancelar');
    await p;
    expect(st().tabs).toHaveLength(1); // se canceló: sigue todo
    const q = st().abrirProyecto('/otro');
    await responder('descartar');
    await q;
    expect(st().tabs).toHaveLength(0);
    expect(getDoc('a.tex')).toBeUndefined();
  });

  it('sin raíz y con varias candidatas no pregunta cuál: se abre la pantalla Proyecto con lo que se detectó', async () => {
    falso({}, { candidates: ['cap_actual.tex', 'libro_fast.tex'], rootFile: null, configSaved: false });
    await st().abrirProyecto('/libro');
    expect(st().dialog).toBeNull();
    expect(st().rootFile).toBeNull();
    await vi.waitFor(() => expect(useProyecto.getState().config).not.toBeNull());
    expect(useProyecto.getState()).toMatchObject({ abierta: true, alAbrir: true, cargando: false });
  });

  it('la raíz que viene de afuera (LV_ROOT) no pregunta nada ni abre la pantalla', async () => {
    const f = falso({}, { candidates: ['a.tex', 'b.tex'], rootFile: null });
    await st().abrirProyecto('/libro', 'b.tex');
    expect(st().dialog).toBeNull();
    expect(st().rootFile).toBe('b.tex');
    expect(f.llamadas).toContain('raiz b.tex');
    expect(useProyecto.getState().abierta).toBe(false);
  });

  it('un proyecto sin latexviewer.json abre la pantalla en lugar de avisar de una configuración inferida', async () => {
    falso({}, { configError: 'latexviewer.json: expected value at line 1', configSaved: false });
    await st().abrirProyecto('/libro');
    expect(st().notice?.nivel).toBe('error');
    expect(st().notice?.texto).toContain('latexviewer.json no se pudo leer');
    await vi.waitFor(() => expect(useProyecto.getState().abierta).toBe(true));
    useProyecto.getState().cerrar();
    falso({}, { configSaved: false });
    await st().abrirProyecto('/libro');
    expect(st().notice).toBeNull();
    await vi.waitFor(() => expect(useProyecto.getState().abierta).toBe(true));
  });

  it('con latexviewer.json y raíz no se abre nada', async () => {
    falso({});
    await st().abrirProyecto('/libro');
    expect(useProyecto.getState().abierta).toBe(false);
  });
});

describe('pantalla Proyecto (F8b)', () => {
  const p = () => useProyecto.getState();

  it('Guardar escribe exactamente lo que se detectó, sin un paso de confirmación, y la app queda con esa configuración', async () => {
    const f = falso({}, { configSaved: false });
    await st().abrirProyecto('/libro');
    await vi.waitFor(() => expect(p().validacion?.ok).toBe(true));
    const propuesta = p().propuesta!;
    await p().pedirGuardar();
    expect(f.control.escrito).toEqual([propuesta.texto]);
    expect(p().abierta).toBe(false);
    expect(st().project?.configSaved).toBe(true);
    expect(st().notice?.texto).toBe('latexviewer.json guardado en la carpeta del proyecto.');
  });

  it('con un latexviewer.json en el disco se parte de él (la detección sólo sugiere) y se guarda lo cambiado', async () => {
    const f = falso({});
    f.control.configEnDisco = '{\n  "root": "cap_actual.tex",\n  "profiles": {\n    "completo": {}\n  }\n}\n';
    await p().abrir();
    expect(p().config?.root).toBe('cap_actual.tex');
    expect(p().alAbrir).toBe(false);
    p().usarDetectado('raiz');
    expect(p().config?.root).toBe('main.tex');
    await p().pedirGuardar();
    expect(f.control.escrito[0]).toContain('"root": "main.tex"');
  });

  it('un latexviewer.json ilegible avisa y se parte de lo detectado', async () => {
    const f = falso({});
    f.control.configEnDisco = '{ esto no es json';
    await p().abrir();
    expect(p().error).toContain('no se pudo leer');
    expect(p().config?.root).toBe('main.tex');
  });

  it('elegir otra raíz: sin archivo en el disco todo se vuelve a detectar; con uno, sólo cambia la raíz', async () => {
    const f = falso({});
    await p().abrir();
    await p().elegirRaiz('cap_actual.tex');
    expect(p().propuesta?.raiz).toBe('cap_actual.tex');
    expect(p().config?.root).toBe('cap_actual.tex');
    p().editar((c) => ({ ...c, engine: 'xelatex' }));
    f.control.configEnDisco = '{ "root": "main.tex", "profiles": { "completo": {} } }';
    await p().abrir();
    p().editar((c) => ({ ...c, engine: 'lualatex' }));
    await p().elegirRaiz('cap_actual.tex');
    expect(p().config).toMatchObject({ root: 'cap_actual.tex', engine: 'lualatex' });
  });

  it('una configuración que no valida no se puede guardar ni confirmar', async () => {
    const f = falso({}, { configSaved: false });
    await st().abrirProyecto('/libro');
    await vi.waitFor(() => expect(p().validacion).not.toBeNull());
    f.control.validacionOk = false;
    p().editar((c) => ({ ...c, profiles: {} }));
    await vi.waitFor(() => expect(p().validacion?.ok).toBe(false));
    await p().pedirGuardar();
    expect(p().paso).toBe('editar');
    expect(await p().confirmarGuardado()).toBe(false);
    expect(f.control.escrito).toEqual([]);
  });

  it('«Avanzado»: un JSON a medio escribir avisa y conserva lo anterior; uno bueno lo reemplaza', async () => {
    falso({});
    await p().abrir();
    const antes = p().config;
    p().editarJson('{ "root": ');
    expect(p().errorDeJson).toContain('no se puede leer');
    expect(p().config).toBe(antes);
    p().editarJson('{ "root": "otro.tex", "profiles": { "a": {} } }');
    expect(p().errorDeJson).toBeNull();
    expect(p().config).toEqual({ root: 'otro.tex', profiles: { a: {} } });
  });

  it('«usar lo detectado» vuelve una sección a lo que detectó el core', async () => {
    falso({});
    await p().abrir();
    p().editar((c) => ({ ...c, profiles: { solo: { label: 'Uno' } } }));
    expect(Object.keys(p().config?.profiles ?? {})).toEqual(['solo']);
    p().usarDetectado('perfiles');
    expect(Object.keys(p().config?.profiles ?? {})).toEqual(['completo']);
  });

  it('probar guarda el resultado y el error si falla', async () => {
    const f = falso({});
    await p().abrir();
    await p().probar('Secciones/Uno');
    expect(p().probada?.listado.omitido).toBe('sin cache');
    f.control.informe = { distribucion: null, ms: 1, hallazgos: [] };
    useProyecto.setState({ config: null });
    await p().probar();
    expect(p().probando).toBe(false);
  });

  it('cerrar descarta lo editado y no escribe nada', async () => {
    const f = falso({});
    await p().abrir();
    p().editar((c) => ({ ...c, engine: 'xelatex' }));
    p().cerrar();
    expect(p()).toMatchObject({ abierta: false, config: null, propuesta: null });
    expect(f.control.escrito).toEqual([]);
  });
});

describe('problemas del build', () => {
  const prob = (o: Partial<Problema>): Problema => ({ severity: 'error', kind: 'tex', message: 'Undefined control sequence.', file: 'cap.tex', line: 2, line_end: null, context: '', ...o });
  const finMal = (o: Partial<Extract<BuildEvent, { type: 'finished' }>> = {}): BuildEvent => ({
    type: 'finished', id: 1, perfil: 'rapido', ok: false, code: 12, cancelled: false, ms: 900, pdf: null, updated: false, passes: 1, truncated_aux: false, repaired: false, errors: 1, warnings: 1, badboxes: 0, ...o,
  });

  it('al terminar un build se piden los problemas del perfil y, si hubo errores, se abre el panel', async () => {
    const f = falso({});
    f.control.problemas = [prob({}), prob({ severity: 'warning', message: 'w', line: 5 })];
    st().manejarEventoBuild(finMal());
    await vi.waitFor(() => expect(st().problems).toHaveLength(2));
    expect(f.llamadas).toContain('problems rapido');
    expect(st().panelInferior).toBe('problemas');
    expect(st().build).toMatchObject({ errors: 1, warnings: 1, badboxes: 0 });
  });

  it('sin errores el panel no se abre solo, y un build cancelado no toca los problemas', async () => {
    const f = falso({});
    f.control.problemas = [prob({ severity: 'warning' })];
    st().manejarEventoBuild(finMal({ ok: true, code: 0, errors: 0 }));
    await vi.waitFor(() => expect(st().problems).toHaveLength(1));
    expect(st().panelInferior).toBe('log');
    f.llamadas.length = 0;
    st().manejarEventoBuild(finMal({ cancelled: true }));
    await new Promise((r) => setTimeout(r, 20));
    expect(f.llamadas).not.toContain('problems rapido');
    expect(st().problems).toHaveLength(1);
  });

  it('ir a un problema abre el archivo y pide la línea; uno sin ubicación avisa', async () => {
    falso({ 'cap.tex': { text: 'a\nb\nc\n' } });
    useApp.setState({ problems: [prob({}), prob({ file: null, line: null, message: 'del sistema' })] });
    await st().irAProblema(0);
    expect(st().activeRel).toBe('cap.tex');
    expect(st().irA).toMatchObject({ rel: 'cap.tex', line: 2 });
    expect(st().problemaIdx).toBe(0);
    useApp.setState({ filtro: { ...FILTRO_INICIAL, avisos: true } });
    await st().irAProblema(1);
    expect(st().notice?.texto).toContain('no tiene una ubicación');
  });

  it('F8 recorre los visibles y da la vuelta; cambiar el filtro reinicia el recorrido', async () => {
    falso({ 'cap.tex': { text: 'a\nb\nc\nd\ne\n' }, 'main.tex': { text: 'x' } });
    useApp.setState({ problems: [prob({ line: 2 }), prob({ severity: 'warning', file: 'main.tex', line: 1, message: 'w' }), prob({ line: 4, message: 'otro' })] });
    await st().saltarProblema(1);
    expect(st().problemaIdx).toBe(0);
    await st().saltarProblema(1);
    expect(st().activeRel).toBe('main.tex');
    await st().saltarProblema(1);
    await st().saltarProblema(1);
    expect(st().problemaIdx).toBe(0);
    await st().saltarProblema(-1);
    expect(st().problemaIdx).toBe(2);
    st().setFiltro({ avisos: false });
    expect(st().problemaIdx).toBe(-1);
    await st().saltarProblema(-1); // desde cero hacia atrás: el último de los visibles
    expect(st().problemaIdx).toBe(1);
  });

  it('sin problemas, F8 avisa en lugar de hacer nada', async () => {
    falso({});
    await st().saltarProblema(1);
    expect(st().notice?.texto).toContain('No hay problemas');
  });

  it('abrir otro proyecto limpia los problemas', async () => {
    falso({});
    useApp.setState({ problems: [prob({})], problemaIdx: 0 });
    await st().abrirProyecto('/otro');
    expect(st().problems).toEqual([]);
    expect(st().problemaIdx).toBe(-1);
  });
});

describe('SyncTeX', () => {
  const rect = (x: number, y: number, w = 40, h = 10, page = 1) => ({ page, x, y, w, h });
  /** Un renglón de PDF: palabras de ancho proporcional, desde la izquierda. */
  const renglon = (texto: string, y: number, x0 = 72) => {
    let x = x0;
    return texto.split(' ').map((t) => {
      const w = t.length * 5;
      const p = { texto: t, clave: t.toLowerCase().normalize('NFD').replace(/\p{M}/gu, ''), page: 1, x, y, w, h: 10 };
      x += w + 3;
      return p;
    });
  };
  const anfitrion = (palabras = renglon('La desnormalización transforma una función', 100)) => {
    const llamadas: string[] = [];
    const host = {
      get etiqueta() {
        return `${st().pdfRel}#${st().pdfEpoch}`;
      },
      palabrasDePagina: async () => palabras,
      resaltar: (pagina: number, rects: unknown[]) => void llamadas.push(`resaltar ${pagina} ${JSON.stringify(rects)}`),
      mostrar: (pagina: number) => void llamadas.push(`mostrar ${pagina}`),
      esVisible: () => true,
      centroVisible: () => ({ page: 1, x: 100, y: 105 }),
    };
    pdfApi.host = host;
    return llamadas;
  };
  const respuesta = (o: Partial<SyncForward> = {}): SyncForward => ({
    perfil: 'parcial',
    pdf: '.lv/build/parcial/lv-parcial.pdf',
    page: 1,
    rects: [rect(60, 98, 480, 12)],
    line_used: 1,
    ...o,
  });
  beforeEach(() => {
    useApp.setState({ pdfRel: '.lv/build/parcial/lv-parcial.pdf', pdfEpoch: 1, seguir: false });
  });
  afterEach(() => {
    pdfApi.host = null;
  });

  it('del cursor al PDF: resalta la palabra del cursor adentro de las cajas', async () => {
    const f = falso({ 'a.tex': { text: 'La desnormalización transforma una función\n' } });
    f.control.forward = respuesta();
    const v = anfitrion();
    await st().abrirArchivo('a.tex');
    await vi.waitFor(() => expect(f.llamadas).toContain('info a.tex'));
    // El cursor dentro de "transforma" (columna 20).
    getDoc('a.tex')!.state = getDoc('a.tex')!.state.update({ selection: { anchor: 20 } }).state;
    expect(await st().irAlPdf()).toBe(true);
    expect(f.llamadas).toContain('forward a.tex:1');
    const r = v.find((x) => x.startsWith('resaltar'))!;
    const rects = JSON.parse(r.slice(r.indexOf('['))) as { x: number; w: number }[];
    expect(rects).toHaveLength(1);
    expect(rects[0].w).toBe('transforma'.length * 5); // la palabra, no toda la caja de 480
    expect(v).toContain('mostrar 1');
  });

  it('si la palabra no aparece resalta las cajas de la línea', async () => {
    const f = falso({ 'a.tex': { text: 'zzzzzzzz\n' } });
    f.control.forward = respuesta();
    const v = anfitrion();
    await st().abrirArchivo('a.tex');
    expect(await st().irAlPdf()).toBe(true);
    const r = v.find((x) => x.startsWith('resaltar'))!;
    expect(JSON.parse(r.slice(r.indexOf('[')))[0].w).toBe(480);
  });

  it('si el PDF elegido no es el que se muestra, lo cambia, avisa y espera a que cargue', async () => {
    const f = falso({ 'a.tex': { text: 'La desnormalización\n' } });
    f.control.forward = respuesta({ perfil: 'rapido', pdf: '.lv/build/rapido/lv-rapido.pdf' });
    anfitrion();
    await st().abrirArchivo('a.tex');
    expect(await st().irAlPdf()).toBe(true);
    expect(st().pdfRel).toBe('.lv/build/rapido/lv-rapido.pdf');
    expect(st().pdfEpoch).toBe(2);
    expect(st().notice?.texto).toContain('«rapido»');
  });

  it('sin cajas para el archivo o con un error del core avisa; en silencio no dice nada', async () => {
    const f = falso({ 'a.tex': { text: 'hola\n' } });
    anfitrion();
    await st().abrirArchivo('a.tex');
    expect(await st().irAlPdf()).toBe(false);
    expect(st().notice?.texto).toContain('compilá');
    useApp.setState({ notice: null });
    expect(await st().irAlPdf({ silencioso: true })).toBe(false);
    expect(st().notice).toBeNull();
    const b = (await import('../backend')).getBackend();
    b.synctexForward = async () => {
      throw new BackendError('other', 'No hay ningún PDF con SyncTeX para este archivo: compilá antes.');
    };
    expect(await st().irAlPdf()).toBe(false);
    expect(st().notice?.nivel).toBe('error');
    expect(f.llamadas.filter((l) => l.startsWith('forward'))).toHaveLength(2);
  });

  it('en silencio no cambia de PDF por su cuenta', async () => {
    const f = falso({ 'a.tex': { text: 'hola\n' } });
    f.control.forward = respuesta({ perfil: 'rapido', pdf: '.lv/build/rapido/lv-rapido.pdf' });
    anfitrion();
    await st().abrirArchivo('a.tex');
    expect(await st().irAlPdf({ silencioso: true })).toBe(false);
    expect(st().pdfRel).toBe('.lv/build/parcial/lv-parcial.pdf');
  });

  it('del PDF al código: abre el archivo y pide seleccionar la palabra clickeada', async () => {
    const f = falso({ 'Secciones/uno.tex': { text: 'primera línea\nLa desnormalización transforma una función\notra más\n' } });
    f.control.inverse = { file: 'Secciones/uno.tex', line: 2, inside: true };
    anfitrion();
    const palabras = renglon('La desnormalización transforma una función', 100);
    const t = palabras.find((p) => p.texto === 'transforma')!;
    expect(await st().irAlCodigo({ page: 1, x: t.x + 3, y: 105 })).toBe(true);
    expect(f.llamadas).toContain(`inverse parcial 1 ${Math.round(t.x + 3)},105`);
    expect(st().activeRel).toBe('Secciones/uno.tex');
    const ir = st().irA!;
    expect(ir.line).toBe(2);
    expect('La desnormalización transforma una función'.slice(ir.inicio, ir.fin)).toBe('transforma');
  });

  it('si el texto no viene de un archivo del proyecto avisa; sin PDF también', async () => {
    const f = falso({});
    f.control.inverse = { file: null, line: 5, inside: true };
    anfitrion();
    expect(await st().irAlCodigo({ page: 1, x: 80, y: 105 })).toBe(false);
    expect(st().notice?.texto).toContain('no viene de un archivo');
    useApp.setState({ pdfRel: null });
    expect(await st().irAlCodigo()).toBe(false);
  });

  it('sin punto usa el centro de lo que se ve', async () => {
    const f = falso({ 'a.tex': { text: 'La desnormalización\n' } });
    f.control.inverse = { file: 'a.tex', line: 1, inside: false };
    anfitrion();
    await st().irAlCodigo();
    expect(f.llamadas).toContain('inverse parcial 1 100,105');
  });

  it('sin punto y con texto seleccionado en el PDF: va al código y selecciona el tramo si las dos puntas caen en la misma línea', async () => {
    const linea = 'La desnormalización transforma una función';
    const f = falso({ 'a.tex': { text: `${linea}\n` } });
    f.control.inverse = { file: 'a.tex', line: 1, inside: true };
    const palabras = renglon(linea, 100);
    anfitrion(palabras);
    const d = palabras.find((p) => p.texto === 'desnormalización')!;
    const u = palabras.find((p) => p.texto === 'función')!;
    (pdfApi.host as { puntoDeSeleccion?: unknown }).puntoDeSeleccion = () => ({
      inicio: { page: 1, x: d.x + 2, y: 105 },
      fin: { page: 1, x: u.x + u.w - 2, y: 105 },
    });
    expect(await st().irAlCodigo()).toBe(true);
    // Parte de la punta de la selección, no del centro de lo que se ve.
    expect(f.llamadas).toContain(`inverse parcial 1 ${Math.round(d.x + 2)},105`);
    const ir = st().irA!;
    expect(linea.slice(ir.inicio, ir.fin)).toBe('desnormalización transforma una función');
  });

  it('con la selección del PDF en otra línea del fuente se queda con la primera palabra', async () => {
    const f = falso({ 'a.tex': { text: 'La desnormalización\notra línea\n' } });
    anfitrion(renglon('La desnormalización', 100));
    // El core contesta la línea 1 para la primera punta y la 2 para la segunda.
    const b = (await import('../backend')).getBackend();
    let n = 0;
    b.synctexInverse = async () => ({ file: 'a.tex', line: ++n === 1 ? 1 : 2, inside: true });
    (pdfApi.host as { puntoDeSeleccion?: unknown }).puntoDeSeleccion = () => ({
      inicio: { page: 1, x: 100, y: 105 },
      fin: { page: 1, x: 110, y: 300 },
    });
    expect(f.llamadas).toBeDefined();
    expect(await st().irAlCodigo()).toBe(true);
    const ir = st().irA!;
    expect(ir.line).toBe(1);
    expect('La desnormalización'.slice(ir.inicio, ir.fin)).not.toContain(' ');
  });

  it('seguir al cursor: se mueve el cursor y 200 ms después el PDF acompaña, sin resaltar y sin avisar', async () => {
    vi.useFakeTimers();
    const f = falso({ 'a.tex': { text: 'La desnormalización transforma\n' } });
    f.control.forward = respuesta();
    const v = anfitrion();
    await st().abrirArchivo('a.tex');
    f.llamadas.length = 0;
    st().alMoverCursor('a.tex'); // apagado: no hace nada
    await vi.advanceTimersByTimeAsync(500);
    expect(f.llamadas).toEqual([]);
    st().setSeguir(true); // al prenderlo ya acompaña
    await vi.advanceTimersByTimeAsync(10);
    expect(f.llamadas).toContain('forward a.tex:1');
    f.llamadas.length = 0;
    st().alMoverCursor('a.tex');
    st().alMoverCursor('a.tex');
    await vi.advanceTimersByTimeAsync(150);
    expect(f.llamadas).toEqual([]);
    await vi.advanceTimersByTimeAsync(100);
    expect(f.llamadas.filter((l) => l.startsWith('forward'))).toHaveLength(1);
    expect(v.some((x) => x.startsWith('resaltar'))).toBe(false);
    expect(v).toContain('mostrar 1');
    expect(st().notice).toBeNull();
  });
});

describe('navegación y búsqueda (F5)', () => {
  const simbolos = (o: Partial<Simbolos> = {}): Simbolos => ({
    labels: [{ name: 'sec:x', file: 'b.tex', line: 2, context: 'Un título' }],
    refs: [{ name: 'sec:x', command: 'ref', file: 'a.tex', line: 1 }],
    cites: [],
    bib: [],
    sections: [
      { level: 1, title: 'Uno', file: 'a.tex', line: 1, label: null },
      { level: 2, title: 'Dos', file: 'b.tex', line: 2, label: 'sec:x' },
    ],
    defs: [],
    files: [
      { rel: 'a.tex', kind: 'tex' },
      { rel: 'b.tex', kind: 'tex' },
      { rel: 'refs.bib', kind: 'bib' },
      { rel: 'fig.png', kind: 'image' },
    ],
    lint: [],
    scan_ms: 1,
    ...o,
  });
  const dosArchivos = () => falso({ 'a.tex': { text: 'Ver \\ref{sec:x} aquí\n' }, 'b.tex': { text: 'x\n\\section{Dos}\\label{sec:x}\n' }, 'refs.bib': { text: '@book{k,}\n' } });

  it('irAUbicacion abre el archivo, pide ir a la línea y recuerda el origen; Alt+← y Alt+→ recorren el historial', async () => {
    dosArchivos();
    await st().abrirArchivo('a.tex');
    await st().irAUbicacion('b.tex', 2);
    expect(st().activeRel).toBe('b.tex');
    expect(st().irA).toMatchObject({ rel: 'b.tex', line: 2 });
    expect(st().historial.map((u) => `${u.rel}:${u.line}`)).toEqual(['a.tex:1', 'b.tex:2']);
    // En la app el editor lleva el cursor a la línea pedida; acá no hay vista, se lo mueve a mano.
    moverCursor('b.tex', 2);
    await st().atras();
    expect(st().activeRel).toBe('a.tex');
    expect(st().irA).toMatchObject({ rel: 'a.tex', line: 1 });
    await st().adelante();
    expect(st().activeRel).toBe('b.tex');
    expect(st().irA).toMatchObject({ rel: 'b.tex', line: 2 });
  });

  it('sin lugar anterior o siguiente avisa y no mueve nada', async () => {
    dosArchivos();
    await st().abrirArchivo('a.tex');
    await st().atras();
    expect(st().notice?.texto).toMatch(/anterior/);
    await st().adelante();
    expect(st().notice?.texto).toMatch(/siguiente/);
    expect(st().activeRel).toBe('a.tex');
  });

  it('un salto nuevo después de volver descarta lo que había "adelante"', async () => {
    dosArchivos();
    await st().abrirArchivo('a.tex');
    await st().irAUbicacion('b.tex', 2);
    await st().atras();
    await st().irAUbicacion('refs.bib', 1);
    expect(st().historial.map((u) => u.rel)).toEqual(['a.tex', 'refs.bib']);
    await st().adelante();
    expect(st().notice?.texto).toMatch(/siguiente/);
  });

  it('F12 sobre un \\ref va a su \\label; sobre algo sin definición avisa', async () => {
    const f = dosArchivos();
    f.control.simbolos = simbolos();
    await st().abrirArchivo('a.tex');
    moverCursor('a.tex', 'Ver \\ref{sec'.length);
    await st().irADefinicion();
    expect(st().activeRel).toBe('b.tex');
    expect(st().irA).toMatchObject({ rel: 'b.tex', line: 2 });
    await st().activar('a.tex');
    moverCursor('a.tex', 1);
    await st().irADefinicion();
    expect(st().notice?.texto).toMatch(/nada con definición/);
  });

  it('abrir archivo rápido lista los .tex y .bib (no las imágenes) con los abiertos primero', async () => {
    const f = dosArchivos();
    f.control.simbolos = simbolos();
    await st().abrirArchivo('b.tex');
    await st().cargarSimbolos();
    await st().abrirArchivoRapido();
    const q = st().quick!;
    expect(q.items.map((i) => i.id)).toEqual(['b.tex', 'a.tex', 'refs.bib']);
    expect(q.items[0].detalle).toBe('abierto');
    await q.items[2].ejecutar();
    expect(st().activeRel).toBe('refs.bib');
  });

  it('ir a sección: del archivo sólo las suyas; del proyecto, todas con sus etiquetas', async () => {
    const f = dosArchivos();
    f.control.simbolos = simbolos();
    await st().abrirArchivo('b.tex');
    await st().irASeccion('archivo');
    expect(st().quick!.items.map((i) => i.titulo.trim())).toEqual(['§ Dos', '⚑ sec:x']);
    await st().irASeccion('proyecto');
    expect(st().quick!.items).toHaveLength(3);
  });

  it('la pestaña siguiente da la vuelta en los dos sentidos', async () => {
    dosArchivos();
    await st().abrirArchivo('a.tex');
    await st().abrirArchivo('b.tex');
    await st().abrirArchivo('refs.bib');
    st().pestanaSiguiente(1);
    expect(st().activeRel).toBe('a.tex');
    st().pestanaSiguiente(-1);
    expect(st().activeRel).toBe('refs.bib');
  });

  it('el lint entra en la lista de problemas que recorre F8, y con la casilla apagada no', async () => {
    const f = dosArchivos();
    f.control.simbolos = simbolos({
      lint: [{ severity: 'warning', kind: 'lint-ref', message: 'sin etiqueta', file: 'a.tex', line: 1, line_end: null, context: '' }],
    });
    await st().cargarSimbolos();
    await st().saltarProblema(1);
    expect(st().activeRel).toBe('a.tex');
    expect(st().problemaIdx).toBe(0);
    st().setFiltro({ lint: false });
    st().cerrarTab('a.tex');
    await st().saltarProblema(1);
    expect(st().activeRel).not.toBe('a.tex');
  });

  it('una búsqueda lenta no pisa el resultado de una más nueva', async () => {
    const f = dosArchivos();
    const resultado = (total: number): ResultadoBusqueda => ({ files: [], total, truncated: false, files_scanned: 1, ms: 1 });
    let soltar: () => void = () => {};
    const primera = new Promise<void>((r) => (soltar = r));
    let n = 0;
    f.control.buscar = async () => {
      n++;
      if (n === 1) {
        await primera;
        return resultado(1);
      }
      return resultado(2);
    };
    st().setBusqueda({ query: 'a' });
    const lenta = st().ejecutarBusqueda();
    st().setBusqueda({ query: 'ab' });
    await st().ejecutarBusqueda();
    soltar();
    await lenta;
    expect(st().busqueda.resultado?.total).toBe(2);
    expect(st().busqueda.buscando).toBe(false);
  });

  it('reemplazar: confirma, guarda lo modificado antes, reemplaza en los archivos pedidos y relee las pestañas abiertas', async () => {
    const f = dosArchivos();
    f.control.busqueda = {
      files: [{ file: 'a.tex', matches: [{ line: 1, col: 0, len: 3, text: 'Ver', offset: 0, replaced: 'Mirar' }] }],
      total: 1,
      truncated: false,
      files_scanned: 2,
      ms: 1,
    };
    f.control.reemplazo = { changed: [['a.tex', 1]], skipped: [], total: 1 };
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: '% ' });
    st().setBusqueda({ query: 'Ver', reemplazo: 'Mirar', conReemplazo: true });
    await st().ejecutarBusqueda();
    f.llamadas.length = 0;
    const p = st().reemplazarTodo();
    await vi.waitFor(() => expect(st().dialog).not.toBeNull());
    expect(st().dialog!.detalle).toMatch(/sin guardar/);
    st().responderDialogo('reemplazar');
    await p;
    // Se guarda antes de tocar el disco, se reemplaza sólo en a.tex y se vuelve a leer la pestaña.
    const orden = f.llamadas.filter((l) => /^(write|reemplazar|read) /.test(l));
    expect(orden).toEqual(['write a.tex', 'reemplazar Ver -> Mirar en a.tex', 'read a.tex']);
    expect(tab('a.tex').dirty).toBe(false);
  });

  it('cancelar la confirmación no toca nada', async () => {
    const f = dosArchivos();
    f.control.busqueda = { files: [{ file: 'a.tex', matches: [{ line: 1, col: 0, len: 3, text: 'Ver', offset: 0, replaced: null }] }], total: 1, truncated: false, files_scanned: 1, ms: 1 };
    st().setBusqueda({ query: 'Ver', reemplazo: 'X', conReemplazo: true });
    await st().ejecutarBusqueda();
    f.llamadas.length = 0;
    const p = st().reemplazarTodo();
    await vi.waitFor(() => expect(st().dialog).not.toBeNull());
    st().responderDialogo('cancelar');
    await p;
    expect(f.llamadas.some((l) => l.startsWith('reemplazar'))).toBe(false);
  });

  it('lo que el core reemplazó en un archivo abierto se relee del disco', async () => {
    const f = dosArchivos();
    await st().abrirArchivo('a.tex');
    f.disco.get('a.tex')!.text = 'Mirar \\ref{sec:x} aquí\n';
    f.disco.get('a.tex')!.version = 'v9';
    await st().recargarPestana('a.tex');
    expect(textoDe('a.tex')).toBe('Mirar \\ref{sec:x} aquí\n');
    expect(tab('a.tex').epoch).toBe(1);
  });
});

describe('cache de figuras (F6)', () => {
  const evento = (e: Record<string, unknown>) => st().manejarEventoBuild(e as unknown as BuildEvent);

  it('cocinar pide la cocción: si arranca marca que está listando y si queda esperando lo dice', async () => {
    const f = falso({});
    await st().cocinarFiguras();
    expect(f.llamadas).toContain('figBake');
    expect(st().figProgreso).toMatchObject({ listando: true });
    f.control.ackBake = { kind: 'queued' };
    useApp.setState({ figProgreso: null });
    await st().cocinarFiguras();
    expect(st().figPendiente).toBe(true);
    expect(st().figProgreso).toBeNull();
  });

  it('los eventos recorren listando, cocinando y terminado, y al terminar se vuelve a pedir el estado', async () => {
    const f = falso({});
    evento({ type: 'bakelisting', id: 1, fase: 'todo' });
    expect(st().figProgreso).toEqual({ fase: 'todo', hechas: 0, total: 0, listando: true });
    evento({ type: 'bakestarted', id: 1, fase: 'todo', total: 4, listed: 9, listing_ms: 800 });
    expect(st().figProgreso).toMatchObject({ total: 4, listando: false });
    evento({ type: 'bakeprogress', id: 1, done: 3, total: 4, name: 'figuras-cache/A-a-2', ok: true });
    expect(st().figProgreso).toMatchObject({ hechas: 3, total: 4 });
    f.llamadas.length = 0;
    evento({ type: 'bakefinished', id: 1, ok: true, cancelled: false, ceded: false, cooked: 4, failed: 0, remaining: 0, ms: 5000, error: null });
    expect(st().figProgreso).toBeNull();
    await vi.waitFor(() => expect(f.llamadas).toContain('figStatus'));
    expect(st().notice).toBeNull();
  });

  it('si alguna figura no salió se avisa y se muestra el panel Figuras con su error', () => {
    falso({});
    evento({ type: 'bakefinished', id: 1, ok: false, cancelled: false, ceded: false, cooked: 3, failed: 2, remaining: 0, ms: 5000, error: null });
    expect(st().notice).toMatchObject({ nivel: 'error' });
    expect(st().notice?.texto).toMatch(/2 figuras no salieron/);
    expect(st().panelIzq).toBe('figuras');
  });

  it('una cocción que no pudo ni empezar da su error', () => {
    falso({});
    evento({ type: 'bakefinished', id: 1, ok: false, cancelled: false, ceded: false, cooked: 0, failed: 0, remaining: 0, ms: 10, error: 'no compila hasta el listado' });
    expect(st().notice?.texto).toContain('no compila hasta el listado');
  });

  it('cancelar la cocción que esperaba un build lo saca de "compilando"; si el build ya empezó, no lo toca', () => {
    falso({});
    useApp.setState({ build: { ...BUILD_INICIAL, status: 'running' } });
    evento({ type: 'bakefinished', id: 1, ok: false, cancelled: true, ceded: false, cooked: 1, failed: 0, remaining: 3, ms: 10, error: null });
    expect(st().build.status).toBe('idle');
    useApp.setState({ build: { ...BUILD_INICIAL, status: 'running', id: 7, perfil: 'rapido' } });
    evento({ type: 'bakefinished', id: 2, ok: false, cancelled: true, ceded: false, cooked: 0, failed: 0, remaining: 0, ms: 10, error: null });
    expect(st().build.status).toBe('running');
  });

  it('la cocción que cede el paso a un build no mata el "compilando" del build', () => {
    falso({});
    useApp.setState({ build: { ...BUILD_INICIAL, status: 'running' } });
    evento({ type: 'bakefinished', id: 1, ok: true, cancelled: false, ceded: true, cooked: 2, failed: 0, remaining: 2, ms: 10, error: null });
    expect(st().build.status).toBe('running');
  });

  it('terminar un build vuelve a pedir el estado de las figuras', async () => {
    const f = falso({});
    f.llamadas.length = 0;
    evento({ type: 'finished', id: 1, perfil: 'parcial', ok: true, code: 0, cancelled: false, ms: 1000, pdf: null, updated: false, passes: 1, truncated_aux: false, repaired: false, errors: 0, warnings: 0, badboxes: 0 });
    await vi.waitFor(() => expect(f.llamadas).toContain('figStatus'));
  });

  it('limpiar muestra qué borraría y sólo borra si se confirma', async () => {
    const f = falso({});
    f.control.limpieza = { archivos: ['figuras-cache/A-a-0.pdf', 'figuras-cache/A-a-0.md5'], bytes: 3 * 1048576, figuras: 1 };
    const p = st().limpiarFiguras();
    await vi.waitFor(() => expect(st().dialog).not.toBeNull());
    expect(st().dialog?.mensaje).toMatch(/2 archivos de 1 figura/);
    expect(st().dialog?.mensaje).toMatch(/3\.0 MB/);
    st().responderDialogo('cancelar');
    await p;
    expect(f.llamadas).not.toContain('figLimpiar');
    const q = st().limpiarFiguras();
    await vi.waitFor(() => expect(st().dialog).not.toBeNull());
    st().responderDialogo('limpiar');
    await q;
    expect(f.llamadas).toContain('figLimpiar');
    expect(st().notice?.texto).toMatch(/Se borraron 2 archivos/);
  });

  it('sin nada que limpiar no pregunta', async () => {
    const f = falso({});
    f.control.limpieza = { archivos: [], bytes: 0, figuras: 0 };
    await st().limpiarFiguras();
    expect(st().dialog).toBeNull();
    expect(f.llamadas).not.toContain('figLimpiar');
    expect(st().notice?.texto).toMatch(/nada que limpiar/);
  });

  it('si la limpieza se niega (no hay lista completa) el motivo llega al aviso', async () => {
    const f = falso({});
    f.control.errorLimpieza = 'todavía no se listaron las figuras del proyecto';
    await st().limpiarFiguras();
    expect(st().notice).toMatchObject({ nivel: 'error' });
    expect(st().notice?.texto).toContain('todavía no se listaron');
  });

  it('mostrar el panel Figuras pide el estado', async () => {
    const f = falso({});
    f.llamadas.length = 0;
    st().mostrarPanel('figuras');
    await vi.waitFor(() => expect(f.llamadas).toContain('figStatus'));
    expect(st().panelIzq).toBe('figuras');
    expect(st().figuras?.sello).toBe('6D06AB');
  });
});

describe('exportar, fragmento, layout y archivos cambiados afuera (F7)', () => {
  const evento = (e: Record<string, unknown>) => st().manejarEventoBuild(e as unknown as BuildEvent);
  const terminado = (perfil: string, o: Record<string, unknown> = {}) => ({
    type: 'finished', id: 3, perfil, ok: true, code: 0, cancelled: false, ms: 1500, pdf: `.lv/build/${perfil}/lv-${perfil}.pdf`, updated: true,
    passes: 1, truncated_aux: false, repaired: false, errors: 0, warnings: 0, badboxes: 0, ...o,
  });

  it('exportar guarda todo antes, pide las partes y muestra el progreso con los eventos del core', async () => {
    const f = falso({ 'a.tex': { text: 'uno\n' } });
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, to: 0, insert: 'x' });
    await st().exportarPartes(['Secciones/Uno', 'Secciones/Dos']);
    expect(f.llamadas).toContain('write a.tex');
    expect(f.llamadas.indexOf('write a.tex')).toBeLessThan(f.llamadas.indexOf('exportar Secciones/Uno,Secciones/Dos'));
    expect(st().exportando).toEqual({ hechas: 0, total: 2, parte: null });
    evento({ type: 'exportprogress', id: 1, done: 1, total: 2, part: 'Secciones/Uno', num: '8', file: '../PDF/Capítulos/08 - Uno.pdf', ok: true });
    expect(st().exportando).toEqual({ hechas: 1, total: 2, parte: 'Secciones/Uno' });
    expect(logBuffer.slice(0, logBuffer.length).join('\n')).toContain('exportada: Secciones/Uno (n.º 8)');
    evento({ type: 'exportfinished', id: 1, ok: true, cancelled: false, exported: 2, failed: 0, ms: 9000, dir: '../PDF/Capítulos', error: null });
    expect(st().exportando).toBeNull();
    expect(st().notice?.texto).toBe('Se exportaron 2 partes a ../PDF/Capítulos.');
  });

  it('exportar sin lista pide todas; con otro trabajo en curso queda esperando; y la exportación cancelada lo dice', async () => {
    const f = falso({});
    f.control.ackExport = { kind: 'queued' };
    await st().exportarPartes();
    expect(f.llamadas).toContain('exportar todas');
    expect(st().exportPendiente).toBe(true);
    expect(st().exportando).toBeNull();
    evento({ type: 'exportstarted', id: 2, total: 3 });
    expect(st().exportPendiente).toBe(false);
    expect(st().exportando).toMatchObject({ total: 3 });
    evento({ type: 'exportfinished', id: 2, ok: false, cancelled: true, exported: 1, failed: 0, ms: 10, dir: 'export', error: null });
    expect(st().notice?.texto).toMatch(/cancelada \(1 de 1 listas\)/);
  });

  it('una exportación con partes que fallaron avisa y ofrece ver sus problemas', async () => {
    const f = falso({});
    evento({ type: 'exportfinished', id: 1, ok: false, cancelled: false, exported: 2, failed: 1, ms: 10, dir: 'export', error: null });
    expect(st().notice).toMatchObject({ nivel: 'error' });
    expect(st().notice?.texto).toMatch(/2 partes y 1 fallaron/);
    st().notice?.accion?.run();
    await vi.waitFor(() => expect(f.llamadas).toContain('problems export'));
  });

  it('exportar la parte activa necesita saber de qué parte es el archivo', async () => {
    const f = falso({});
    await st().exportarParteActiva();
    expect(st().notice?.texto).toMatch(/no es de ninguna parte/);
    expect(f.llamadas.some((l) => l.startsWith('exportar'))).toBe(false);
    useApp.setState({ parteActiva: 'Secciones/Dos' });
    await st().exportarParteActiva();
    expect(f.llamadas).toContain('exportar Secciones/Dos');
  });

  it('los builds de servicio no tocan el estado del build ni el PDF del documento', () => {
    falso({});
    useApp.setState({ pdfRel: '.lv/build/parcial/lv-parcial.pdf', pdfEpoch: 4, build: { ...BUILD_INICIAL, status: 'ok', perfil: 'parcial' } });
    evento({ type: 'started', id: 9, perfil: 'export', cmdline: 'latexmk export', retry: false });
    expect(st().build).toMatchObject({ status: 'ok', perfil: 'parcial' });
    evento({ type: 'started', id: 10, perfil: 'fragmento', cmdline: 'latexmk fragmento', retry: false });
    expect(st().build).toMatchObject({ status: 'ok', perfil: 'parcial' });
    expect(st().fragmento.estado).toBe('compilando');
    evento(terminado('fragmento'));
    expect(st().fragmento).toMatchObject({ estado: 'ok', pdfRel: '.lv/build/fragmento/lv-fragmento.pdf', epoch: 1 });
    expect(st().pdfRel).toBe('.lv/build/parcial/lv-parcial.pdf');
    expect(st().pdfEpoch).toBe(4);
    expect(st().build.perfil).toBe('parcial');
  });

  it('un fragmento que no compila conserva el último bueno y lo dice', () => {
    falso({});
    useApp.setState({ fragmento: { estado: 'compilando', pdfRel: '.lv/build/fragmento/lv-fragmento.pdf', epoch: 2, ms: 1 } });
    evento(terminado('fragmento', { ok: false, code: 12, pdf: null, errors: 1 }));
    expect(st().fragmento).toMatchObject({ estado: 'fail', pdfRel: '.lv/build/fragmento/lv-fragmento.pdf', epoch: 2 });
    expect(st().notice).toMatchObject({ nivel: 'error' });
  });

  it('la vista previa compila lo que rodea al cursor y muestra la pestaña Fragmento', async () => {
    const f = falso({ 'a.tex': { text: 'Antes.\n\n\\begin{figure}\n\\begin{tikzpicture}\\draw (0,0);\\end{tikzpicture}\n\\caption{c}\n\\end{figure}\n' } });
    await st().abrirArchivo('a.tex');
    moverCursor('a.tex', textoDe('a.tex')!.indexOf('\\draw'));
    await st().previsualizarFragmento();
    const pedido = f.llamadas.find((l) => l.startsWith('fragmento '))!;
    expect(JSON.parse(pedido.slice('fragmento '.length))).toBe('\\begin{figure}\n\\begin{tikzpicture}\\draw (0,0);\\end{tikzpicture}\n\\caption{c}\n\\end{figure}');
    expect(st().vistaPdf).toBe('fragmento');
    expect(st().fragmento.estado).toBe('compilando');
    // Con selección, la selección.
    moverCursor('a.tex', 0, 6);
    await st().previsualizarFragmento();
    expect(f.llamadas.filter((l) => l.startsWith('fragmento ')).pop()).toBe('fragmento "Antes."');
    st().mostrarVista('pdf');
    expect(st().vistaPdf).toBe('pdf');
  });

  it('sin archivo abierto la vista previa avisa y no pide nada', async () => {
    const f = falso({});
    await st().previsualizarFragmento();
    expect(st().notice?.texto).toMatch(/Abrí un archivo/);
    expect(f.llamadas.some((l) => l.startsWith('fragmento'))).toBe(false);
  });

  it('Ctrl+barra invertida recorre los tres layouts y vuelve al primero', () => {
    falso({});
    const visto = [st().layout];
    for (let i = 0; i < 3; i++) {
      st().alternarLayout();
      visto.push(st().layout);
    }
    expect(visto).toEqual(['ambos', 'editor', 'pdf', 'ambos']);
  });

  it('el menú de compilación lista perfiles, variables, exportar, cocinar y limpiar, y las variables se eligen en un segundo selector', async () => {
    const f = falso({});
    useApp.setState({ parteActiva: 'Secciones/Uno' });
    st().menuCompilacion();
    const items = st().quick!.items;
    const ids = items.map((i) => i.id);
    expect(ids).toEqual(expect.arrayContaining(['compilar', 'perfil:parcial', 'perfil:rapido', 'perfil:completo', 'variable:OpcionPie', 'exportar:activa', 'exportar:elegidas', 'exportar:todas', 'fragmento', 'cocinar', 'limpiar', 'publicar']));
    expect(items.find((i) => i.id === 'perfil:parcial')!.titulo).toContain('✓');
    expect(items.find((i) => i.id === 'variable:OpcionPie')!.titulo).toBe('Variable: Pie = completo');
    // Elegir una variable abre otro selector con sus opciones y la aplica.
    await items.find((i) => i.id === 'variable:OpcionPie')!.ejecutar();
    expect(st().quick!.items.map((i) => i.id)).toEqual(['completo', 'autor']);
    await st().quick!.items[1].ejecutar();
    expect(f.llamadas).toContain('estado {"variables":{"OpcionPie":"autor"}}');
    // Exportar desde el menú: la parte del archivo abierto.
    st().menuCompilacion();
    await st().quick!.items.find((i) => i.id === 'exportar:activa')!.ejecutar();
    expect(f.llamadas).toContain('exportar Secciones/Uno');
  });

  it('el menú de compilación abre la carpeta de los PDF publicados y la de los exportados', async () => {
    const f = falso({});
    st().menuCompilacion();
    const items = st().quick!.items;
    expect(items.find((i) => i.id === 'carpeta:publicar')!.detalle).toBe('../PDF');
    await items.find((i) => i.id === 'carpeta:publicar')!.ejecutar();
    await items.find((i) => i.id === 'carpeta:exportar')!.ejecutar();
    expect(f.llamadas).toEqual(expect.arrayContaining(['carpeta publicar', 'carpeta exportar']));
  });

  describe('imágenes en borrador', () => {
    const conEntrega = (draftImages: boolean) => {
      const base = infoDe({ state: { ...estadoBase(), draftImages } });
      base.config!.profiles.push(perfil('entrega', { publica: true }), perfil('mio', { draftImages: true }));
      return base;
    };

    it('el interruptor aplica el patch, avisa, y el ítem del menú lo invierte', async () => {
      const f = falso({});
      await st().setDraftImages(true);
      expect(f.llamadas).toContain('estado {"draftImages":true}');
      expect(st().notice?.texto).toContain('Imágenes en borrador');
      st().menuCompilacion();
      const item = st().quick!.items.find((i) => i.id === 'borrador')!;
      expect(item.titulo).toBe('Imágenes: completas');
      await item.ejecutar();
      expect(f.llamadas).toContain('estado {"draftImages":false}');
      expect(st().notice?.texto).toContain('Imágenes completas');
    });

    it('perfilEnBorrador: el interruptor no vale para la exportación ni para lo que se publica, el perfil con draftImages sí siempre', async () => {
      const { perfilEnBorrador } = await import('./app');
      const apagado = conEntrega(false);
      expect(perfilEnBorrador(apagado, 'rapido')).toBe(false);
      expect(perfilEnBorrador(apagado, 'mio')).toBe(true);
      const prendido = conEntrega(true);
      expect(perfilEnBorrador(prendido, 'rapido')).toBe(true);
      expect(perfilEnBorrador(prendido, 'entrega')).toBe(false);
      expect(perfilEnBorrador(prendido, 'mio')).toBe(true);
      expect(perfilEnBorrador(prendido, 'no-existe')).toBe(false);
      expect(perfilEnBorrador(null, 'rapido')).toBe(false);
    });
  });

  describe('archivos cambiados afuera', () => {
    it('una pestaña sin cambios se recarga sola; la que tiene cambios avisa y no se pisa', async () => {
      const f = falso({ 'a.tex': { text: 'uno\n' }, 'b.tex': { text: 'dos\n' } });
      await st().abrirArchivo('a.tex');
      await st().abrirArchivo('b.tex');
      editarDoc('b.tex', { from: 0, to: 0, insert: 'mío ' });
      f.disco.set('a.tex', { text: 'uno cambiado\n', version: 'v7', eol: 'lf', bom: false, mixed: false });
      f.disco.set('b.tex', { text: 'dos cambiado\n', version: 'v8', eol: 'lf', bom: false, mixed: false });
      await st().alCambiarAfuera(['a.tex', 'b.tex']);
      expect(textoDe('a.tex')).toBe('uno cambiado\n');
      expect(tab('a.tex').dirty).toBe(false);
      expect(textoDe('b.tex')).toBe('mío dos\n');
      expect(st().notice).toMatchObject({ nivel: 'error' });
      expect(st().notice?.texto).toMatch(/b\.tex cambió afuera y tiene cambios sin guardar/);
      expect(st().arbolEpoch).toBe(1);
    });

    it('lo que acaba de escribir la propia aplicación (la versión que ya conoce la pestaña) no es un cambio de afuera', async () => {
      const f = falso({ 'a.tex': { text: 'uno\n' } });
      await st().abrirArchivo('a.tex');
      editarDoc('a.tex', { from: 0, to: 0, insert: 'x' });
      await st().guardar('a.tex');
      f.llamadas.length = 0;
      Object.assign(f.estado, { autoCompile: 'save' });
      useApp.setState({ project: infoDe({ state: { ...f.estado } }) });
      await st().alCambiarAfuera(['a.tex']);
      expect(f.llamadas).toEqual(['read a.tex']);
      expect(st().notice).toBeNull();
    });

    it('un archivo que usa el perfil activo compila una sola vez aunque cambien varios', async () => {
      const f = falso({});
      Object.assign(f.estado, { autoCompile: 'save' });
      useApp.setState({ project: infoDe({ state: { ...f.estado } }) });
      await st().alCambiarAfuera(['Secciones/fig_a.tex', 'Secciones/fig_b.tex', 'Herramientas/preview/x.tex']);
      expect(f.llamadas.filter((l) => l === 'build')).toHaveLength(1);
    });

    it('con la compilación en manual, o si el archivo no entra en el perfil, no se compila', async () => {
      const f = falso({});
      await st().alCambiarAfuera(['Secciones/fig_a.tex']);
      expect(f.llamadas).not.toContain('build');
      Object.assign(f.estado, { autoCompile: 'save' });
      useApp.setState({ project: infoDe({ state: { ...f.estado } }) });
      f.control.fileInfo = () => ({ part: 'Secciones/Dos', affects: 'other_part', profileAuto: true, partsChanged: false });
      await st().alCambiarAfuera(['Secciones/Dos.tex']);
      f.control.fileInfo = () => ({ part: null, affects: 'unrelated', profileAuto: true, partsChanged: false });
      await st().alCambiarAfuera(['notas.txt']);
      expect(f.llamadas).not.toContain('build');
    });

    it('un .tex que cambia las partes vuelve a leer el proyecto', async () => {
      const f = falso({});
      f.llamadas.length = 0;
      await st().alCambiarAfuera(['libro_cuerpo.tex']);
      expect(f.llamadas).toContain('getProject');
    });

    it('una ráfaga enorme (un git checkout) compila sin preguntar archivo por archivo', async () => {
      const f = falso({});
      Object.assign(f.estado, { autoCompile: 'save' });
      useApp.setState({ project: infoDe({ state: { ...f.estado } }) });
      f.llamadas.length = 0;
      await st().alCambiarAfuera(Array.from({ length: 40 }, (_, i) => `x${i}.tex`));
      expect(f.llamadas.filter((l) => l.startsWith('info '))).toHaveLength(0);
      expect(f.llamadas.filter((l) => l === 'build')).toHaveLength(1);
    });
  });
});

describe('la barra mientras compila (F8b)', () => {
  const empezar = (id = 1, perfil = 'parcial') => {
    st().manejarEventoBuild({ type: 'started', id, perfil, cmdline: 'latexmk', retry: false });
  };

  it('guarda la herramienta que corre y la figura que pgf lanza, y los borra al empezar otro build', async () => {
    falso({});
    await st().init();
    empezar();
    expect(st().build).toMatchObject({ status: 'running', paso: null, figura: null });
    st().manejarEventoBuild({ type: 'pass', id: 1, n: 1, tool: 'pdflatex' });
    st().manejarEventoBuild({ type: 'figure', id: 1, n: 3, name: 'C7C9E7-butter-2' });
    expect(st().build).toMatchObject({ paso: { tool: 'pdflatex', n: 1 }, figura: { n: 3, nombre: 'C7C9E7-butter-2' } });
    st().manejarEventoBuild({ type: 'pass', id: 1, n: 1, tool: 'biber' });
    expect(st().build.paso).toEqual({ tool: 'biber', n: 1 });
    st().manejarEventoBuild({ type: 'finished', id: 1, perfil: 'parcial', ok: true, code: 0, cancelled: false, ms: 1500, pdf: 'x.pdf', updated: true, passes: 2, truncated_aux: false, repaired: false, errors: 0, warnings: 0, badboxes: 0 });
    empezar(2);
    expect(st().build).toMatchObject({ id: 2, paso: null, figura: null });
  });

  it('ignora los pasos de un build que no es el que está en curso (una exportación, un fragmento)', async () => {
    falso({});
    await st().init();
    empezar();
    st().manejarEventoBuild({ type: 'pass', id: 99, n: 1, tool: 'pdflatex' });
    st().manejarEventoBuild({ type: 'figure', id: 99, n: 1, name: 'AAAAAA-x-0' });
    expect(st().build).toMatchObject({ paso: null, figura: null });
  });

  it('el tiempo cuenta desde el pedido (incluye la cocción previa) y un reintento no lo reinicia', async () => {
    falso({});
    await st().init();
    await st().compilar('parcial', { guardar: false });
    const pedido = st().build.inicioMs;
    expect(pedido).not.toBeNull();
    await new Promise((r) => setTimeout(r, 20));
    empezar();
    expect(st().build.inicioMs).toBe(pedido);
    st().manejarEventoBuild({ type: 'started', id: 2, perfil: 'parcial', cmdline: 'latexmk', retry: true });
    expect(st().build.inicioMs).toBe(pedido);
  });

  it('lee el típico de cada perfil al abrir y cuando termina un build', async () => {
    const f = falso({});
    f.control.tipicos = { parcial: { ms: 5000, pasadas: 2, n: 4 } };
    await st().init();
    await st().cargarTipicos();
    expect(st().tipicos.parcial).toEqual({ ms: 5000, pasadas: 2, n: 4 });
    f.control.tipicos = { parcial: { ms: 6000, pasadas: 2, n: 5 } };
    empezar();
    st().manejarEventoBuild({ type: 'finished', id: 1, perfil: 'parcial', ok: true, code: 0, cancelled: false, ms: 1500, pdf: 'x.pdf', updated: true, passes: 2, truncated_aux: false, repaired: false, errors: 0, warnings: 0, badboxes: 0 });
    await vi.waitFor(() => expect(st().tipicos.parcial.ms).toBe(6000));
  });
});

describe('historial, doctor y procesos (F8)', () => {
  const huerfano = (pid: number): ProcesoTex => ({ pid, nombre: 'pdflatex.exe', cwd: '/p', linea: 'pdflatex main.tex' });

  it('los builds que disparan un guardado o un archivo cambiado afuera dicen qué los pidió; uno a mano, no', async () => {
    const f = falso({ 'Secciones/Uno.tex': { text: 'uno\n' } });
    Object.assign(f.estado, { autoCompile: 'save' });
    useApp.setState({ project: infoDe({ state: { ...f.estado } }) });
    await st().abrirArchivo('Secciones/Uno.tex');
    editarDoc('Secciones/Uno.tex', { from: 0, to: 0, insert: 'x' });
    await st().guardar('Secciones/Uno.tex');
    await st().alCambiarAfuera(['Secciones/fig_a.tex', 'Herramientas/b.tex', 'c.tex', 'd.tex', 'e.tex']);
    await st().compilar(undefined, { guardar: false });
    expect(f.control.motivos).toEqual(['guardado: Secciones/Uno.tex', 'afuera: Secciones/fig_a.tex, Herramientas/b.tex, c.tex y 2 más', undefined]);
  });

  it('el panel Historial lee el historial al mostrarse y se actualiza cuando termina un trabajo con el panel a la vista', async () => {
    const f = falso({});
    const linea = (tipo: EntradaHistorial['tipo']): EntradaHistorial => ({ inicioMs: 1, tipo, motivo: 'manual', ms: 10, ok: true, pistas: ['faltaba el .fdb_latexmk'] });
    f.control.historial = [linea('build')];
    await st().init(); // engancha los eventos del backend
    st().mostrarPanelInferior('historial');
    await vi.waitFor(() => expect(st().historialTrabajos).toHaveLength(1));
    f.control.historial = [linea('bake'), linea('build')];
    st().manejarEventoBuild({ type: 'bakefinished', ok: true, cancelled: false, cooked: 3, failed: 0, ms: 5, error: null } as unknown as BuildEvent);
    await vi.waitFor(() => expect(st().historialTrabajos).toHaveLength(2));
    // Con otro panel a la vista no se lee (no hay nadie mirando).
    st().mostrarPanelInferior('log');
    f.control.historial = [linea('export')];
    st().manejarEventoBuild({ type: 'bakefinished', ok: true, cancelled: false, cooked: 0, failed: 0, ms: 5, error: null } as unknown as BuildEvent);
    await Promise.resolve();
    expect(st().historialTrabajos).toHaveLength(2);
  });

  it('al abrir un proyecto el doctor corre y, si hay avisos, lo dice con un botón que abre el panel; sin hallazgos no molesta', async () => {
    const f = falso({});
    await st().abrirProyecto('/p');
    await vi.waitFor(() => expect(st().doctor).not.toBeNull());
    expect(st().notice).toBeNull();
    f.control.informe = {
      distribucion: 'MiKTeX 26.5',
      ms: 9,
      hallazgos: [
        { id: 'variable-def', nivel: 'aviso', titulo: '«\\OpcionPie» se define sin guardia', detalle: '', arreglo: 'usar \\ifdefined', archivo: 'structure.tex', linea: 3 },
        { id: 'perl', nivel: 'error', titulo: 'No se encontró perl', detalle: '', arreglo: null, archivo: null, linea: null },
      ],
    };
    useApp.setState({ doctor: null });
    await st().correrDoctor({ alAbrir: true });
    expect(st().notice).toMatchObject({ nivel: 'error', texto: 'El doctor encontró 1 error y 1 aviso.' });
    st().notice!.accion!.run();
    expect(st().panelInferior).toBe('doctor');
    // A pedido abre el panel directamente, incluso sin hallazgos.
    useApp.setState({ panelInferior: 'log', notice: null });
    f.control.informe = { distribucion: null, ms: 1, hallazgos: [] };
    await st().correrDoctor();
    expect(st().panelInferior).toBe('doctor');
    expect(st().doctorCorriendo).toBe(false);
  });

  it('al abrir con procesos de TeX huérfanos pregunta y, si se acepta, los mata; si se deja, no', async () => {
    const f = falso({});
    f.control.huerfanos = [huerfano(41), huerfano(42)];
    const abriendo = st().abrirProyecto('/p');
    await responder('matar');
    await abriendo;
    await vi.waitFor(() => expect(f.control.muertos).toEqual([41, 42]));
    expect(st().dialog).toBeNull();
    f.control.muertos.length = 0;
    const otra = st().revisarHuerfanos();
    await responder('dejar');
    await otra;
    expect(f.control.muertos).toEqual([]);
  });

  it('un proyecto abierto en otra instancia lo avisa y no pregunta por huérfanos ni corre el doctor', async () => {
    const f = falso({}, { lockedBy: 777 });
    f.control.huerfanos = [huerfano(41)];
    await st().abrirProyecto('/p');
    expect(st().notice).toMatchObject({ nivel: 'error' });
    expect(st().notice?.texto).toMatch(/otra instancia.*PID 777/);
    expect(st().dialog).toBeNull();
    expect(st().doctor).toBeNull();
  });
});

describe('TeXpresso', () => {
  const tp = (f: { llamadas: string[] }) => f.llamadas.filter((l) => l.startsWith('tp '));

  it('apagado no le manda nada al editar ni al mover el cursor', async () => {
    vi.useFakeTimers();
    const f = falso({ 'a.tex': { text: 'uno\ndos\n' } });
    await iniciarTexpresso();
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    st().alEditar('a.tex');
    st().alMoverCursor('a.tex');
    await vi.advanceTimersByTimeAsync(1000);
    expect(tp(f)).toEqual([]);
  });

  it('prendido manda el texto con debounce, el cursor y el cierre; un clic en su ventana lleva al código', async () => {
    vi.useFakeTimers();
    const f = falso({ 'a.tex': { text: 'uno\ndos\n' }, 'b.png': { text: '' } });
    await iniciarTexpresso();
    await st().abrirArchivo('a.tex');
    editarDoc('a.tex', { from: 0, insert: 'X' });
    await useTexpresso.getState().activar(true);
    // Al prender se manda lo que el editor tiene sin guardar y dónde está el cursor.
    expect(tp(f)).toEqual(['tp activar true', 'tp buffer a.tex "Xuno\\ndos\\n"', 'tp posicion a.tex:1']);
    f.llamadas.length = 0;
    // Tres teclas seguidas: un solo envío, con el último texto.
    for (const c of ['a', 'b', 'c']) {
      editarDoc('a.tex', { from: 0, insert: c });
      st().alEditar('a.tex');
      await vi.advanceTimersByTimeAsync(DEBOUNCE_TEXTO_MS / 3);
    }
    await vi.advanceTimersByTimeAsync(DEBOUNCE_TEXTO_MS);
    expect(tp(f)).toEqual(['tp buffer a.tex "cbaXuno\\ndos\\n"']);
    f.llamadas.length = 0;
    moverCursor('a.tex', 9);
    st().alMoverCursor('a.tex');
    await vi.advanceTimersByTimeAsync(DEBOUNCE_CURSOR_MS + 10);
    expect(tp(f)).toEqual(['tp posicion a.tex:2']);
    // Una imagen no se le manda.
    f.llamadas.length = 0;
    st().alEditar('b.png');
    await vi.advanceTimersByTimeAsync(DEBOUNCE_TEXTO_MS * 2);
    expect(tp(f)).toEqual([]);
    // Cerrar la pestaña (sin guardar) lo devuelve al disco.
    vi.useRealTimers();
    const cerrar = st().cerrarTab('a.tex');
    await responder('descartar');
    await cerrar;
    expect(tp(f)).toContain('tp cerrar a.tex');
    // Un clic en la ventana de TeXpresso abre el archivo en esa línea.
    f.control.emitirTexpresso({ type: 'sync', rel: 'a.tex', line: 2 });
    await vi.waitFor(() => expect(st().activeRel).toBe('a.tex'));
    expect(st().activeRel).toBe('a.tex');
    expect(st().irA).toMatchObject({ rel: 'a.tex', line: 2 });
  });

  it('el texto viaja con el EOL y el BOM del archivo (TeXpresso compara contra los bytes del disco)', async () => {
    vi.useFakeTimers();
    const f = falso({ 'crlf.tex': { text: 'uno\ndos\n', eol: 'crlf', bom: true } });
    await iniciarTexpresso();
    await st().abrirArchivo('crlf.tex');
    await useTexpresso.getState().activar(true);
    f.llamadas.length = 0;
    editarDoc('crlf.tex', { from: 3, insert: '!' });
    st().alEditar('crlf.tex');
    await vi.advanceTimersByTimeAsync(DEBOUNCE_TEXTO_MS * 2);
    expect(tp(f)).toEqual([`tp buffer crlf.tex ${JSON.stringify('﻿uno!\r\ndos\r\n')}`]);
  });

  it('el estado que llega del core actualiza el botón, y lo mostrado se anota', async () => {
    const f = falso({});
    await iniciarTexpresso();
    f.control.emitirTexpresso({ type: 'estado', vista: { ...f.control.texpresso.alPrender, fase: 'listo', ultimoMs: 12 } });
    expect(useTexpresso.getState().vista?.fase).toBe('listo');
    f.control.emitirTexpresso({ type: 'actualizado', ms: 15, rel: 'a.tex' });
    expect(useTexpresso.getState().tiempos).toEqual([15]);
  });
});
