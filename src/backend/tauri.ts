// La única implementación de `Backend` y el único archivo que usa `invoke` y `listen`. Nada de `invoke` desparramado
// por la interfaz (3.2 del plan).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { BackendError, type Backend, type BuildEvent, type ErrorKind, type EventoTexpresso, type LaunchOptions, type VistaTexpresso } from './types';

interface ErrorInfo {
  kind: ErrorKind;
  message: string;
  disk_version?: string;
}

const esErrorInfo = (e: unknown): e is ErrorInfo =>
  typeof e === 'object' && e !== null && 'kind' in e && 'message' in e;

/** Los comandos devuelven `ErrorInfo` serializado; se lo convierte en un `Error` de verdad. */
async function llamar<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (esErrorInfo(e)) throw new BackendError(e.kind, e.message, e.disk_version);
    throw new BackendError('other', String(e));
  }
}

export const tauriBackend: Backend = {
  async launchOptions() {
    const o = await llamar<{
      open: string | null;
      root: string | null;
      smoke: boolean;
      smoke_mode: 'f1' | 'f2' | 'f3' | 'f4' | 'f5' | 'f6' | 'f7' | 'f8' | 'f8b' | 'f8c' | 'texpresso';
      smoke_file: string | null;
      smoke_pausa_ms: number;
    }>('launch_options');
    const l: LaunchOptions = { open: o.open, root: o.root, smoke: o.smoke, smokeMode: o.smoke_mode, smokeFile: o.smoke_file, smokePausaMs: o.smoke_pausa_ms };
    return l;
  },
  pickFolder: () => llamar('pick_folder'),
  openProject: (path) => llamar('open_project', { path }),
  getProject: () => llamar('get_project'),
  setRootFile: (rel) => llamar('set_root_file', { rel }),
  setState: (patch) => llamar('set_state', { patch }),
  saveConfig: () => llamar('save_config'),
  fileInfo: (rel) => llamar('file_info', { rel }),
  listDir: (rel) => llamar('list_dir', { rel }),
  readFile: (rel) => llamar('read_file', { rel }),
  writeFile: (rel, text, expectedVersion) => llamar('write_file', { rel, text, expectedVersion }),
  async readPdf(rel) {
    return new Uint8Array(await llamar<ArrayBuffer>('read_pdf', { rel }));
  },
  build: (profile, reason) => llamar('build', { profile: profile ?? null, reason: reason ?? null }),
  cancelBuild: () => llamar('cancel_build'),
  history: (limit) => llamar('history', { limit: limit ?? null }),
  typicalTimes: () => llamar('typical_times'),
  detectProject: (root) => llamar('detect_project', { root: root ?? null }),
  configOnDisk: () => llamar('config_on_disk'),
  validateConfig: (text) => llamar('validate_config', { text }),
  writeConfig: (text) => llamar('write_config', { text }),
  probeConfig: (text, part) => llamar('probe_config', { text, part: part ?? null }),
  runDoctor: () => llamar('run_doctor'),
  orphans: () => llamar('orphans'),
  killOrphans: (pids) => llamar('kill_orphans', { pids }),
  problems: (profile) => llamar('problems', { profile }),
  synctexForward: (rel, line) => llamar('synctex_forward', { rel, line }),
  synctexInverse: (profile, page, x, y) => llamar('synctex_inverse', { profile, page, x, y }),
  getSymbols: () => llamar('get_symbols'),
  figuresStatus: () => llamar('figures_status'),
  figuresBake: () => llamar('figures_bake'),
  figuresCleanPreview: () => llamar('figures_clean_preview'),
  figuresClean: () => llamar('figures_clean'),
  searchProject: (options, replacement) => llamar('search_project', { options, replacement: replacement ?? null }),
  replaceProject: (options, replacement, files) => llamar('replace_project', { options, replacement, files: files ?? null }),
  readUserConfig: (name) => llamar('read_user_config', { name }),
  writeUserConfig: (name, text) => llamar('write_user_config', { name, text }),
  publish: (profile) => llamar('publish', { profile }),
  openOutputDir: (cual) => llamar('open_output_dir', { cual }),
  exportParts: (parts) => llamar('export_parts', { parts: parts ?? null }),
  previewFragment: (text) => llamar('preview_fragment', { text }),
  texpressoEstado: () => llamar('texpresso_estado'),
  texpressoActivar: (on) => llamar('texpresso_activar', { on }),
  texpressoBuffer: (rel, text) => llamar('texpresso_buffer', { rel, text }),
  texpressoCerrar: (rel) => llamar('texpresso_cerrar', { rel }),
  texpressoPosicion: (rel, line) => llamar('texpresso_posicion', { rel, line }),
  texpressoVivo: (ancho) => llamar('texpresso_vivo', { ancho }),
  texpressoVivoClic: (x, y) => llamar('texpresso_vivo_clic', { x, y }),
  texpressoPaginaVivo: (delta) => llamar('texpresso_pagina_vivo', { delta }),
  async onTexpresso(cb) {
    // `texpresso:estado` lleva la vista sola; los otros ya vienen con su `type`.
    const d = await Promise.all([
      listen<VistaTexpresso>('texpresso:estado', (e) => cb({ type: 'estado', vista: e.payload })),
      listen<EventoTexpresso>('texpresso:sync', (e) => cb(e.payload)),
      listen<EventoTexpresso>('texpresso:actualizado', (e) => cb(e.payload)),
      listen<EventoTexpresso>('texpresso:pagina', (e) => cb(e.payload)),
    ]);
    return () => d.forEach((f) => f());
  },
  async onFsChanged(cb) {
    return listen<{ files: string[] }>('fs:changed', (e) => cb(e.payload.files));
  },
  async onBuildEvent(cb) {
    const nombres = [
      'build:queued',
      'build:started',
      'build:log',
      'build:pass',
      'build:figure',
      'build:finished',
      'build:error',
      'figures:queued',
      'figures:listing',
      'figures:started',
      'figures:progress',
      'figures:finished',
      'export:queued',
      'export:started',
      'export:progress',
      'export:finished',
    ];
    const desuscribir = await Promise.all(nombres.map((n) => listen<BuildEvent>(n, (e) => cb(e.payload))));
    return () => desuscribir.forEach((d) => d());
  },
  async onCloseRequested(permitir) {
    return getCurrentWindow().onCloseRequested(async (evento) => {
      if (!(await permitir())) evento.preventDefault();
    });
  },
  exitApp: () => llamar('exit_app'),
  async smokeReadBytes(rel) {
    return new Uint8Array(await llamar<ArrayBuffer>('smoke_read_bytes', { rel }));
  },
  smokeMtime: (rel) => llamar('smoke_mtime', { rel }),
  writeSmokeResult: (json) => llamar('write_smoke_result', { json }),
  smokeTexpressoColgar: () => llamar('smoke_texpresso_colgar'),
  smokeTexpressoProcesos: () => llamar('smoke_texpresso_procesos'),
};
