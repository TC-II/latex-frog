// Edición del `latexviewer.json` que la pantalla Proyecto arma (F8b): funciones puras sobre el objeto JSON, sin React ni backend. Cada una
// devuelve un objeto nuevo (no muta el que recibe) y conserva todo lo que no toca, incluidas las claves que la app no conoce.
import type { UsoDeFlag } from '../backend';

export interface PerfilJson {
  label?: string;
  partial?: boolean;
  externalize?: boolean;
  cacheOnly?: boolean;
  parts?: 'selection' | 'all' | 'one' | 'current' | 'list';
  cleanBefore?: boolean;
  autoCompile?: boolean;
  hidden?: boolean;
  draftImages?: boolean;
  /** Partes fijas: todas las de `list`, o las que se suman a la actual en `current`. */
  partList?: string[];
  /** Piezas del documento que el perfil saca (`toc`, `bibliography`, `index`, `pdf:<archivo>`). */
  omit?: string[];
  color?: string;
  vars?: Record<string, string>;
  output?: { dir: string; pattern: string; perPart?: boolean };
  compileOn?: 'save' | 'idle';
}

export interface VariableJson {
  label?: string;
  choices?: string[];
  default?: string;
}

export interface ConfigJson {
  root?: string;
  engine?: string;
  parts?: { commands?: string[] };
  flags?: { partial?: string[]; externalize?: string[]; figureList?: string[] };
  figureCache?: Record<string, unknown>;
  variables?: Record<string, VariableJson>;
  profiles?: Record<string, PerfilJson>;
  publish?: { dir: string; names?: Record<string, string> };
  export?: { dir?: string; pattern?: string };
  [clave: string]: unknown;
}

/** Las partes de la configuración que la pantalla ofrece «usar lo detectado» una por una. */
export type SeccionDeConfig = 'raiz' | 'partes' | 'flags' | 'figuras' | 'perfiles' | 'salidas' | 'variables';

const clonar = <T>(x: T): T => JSON.parse(JSON.stringify(x)) as T;

export function parsear(texto: string): ConfigJson | null {
  try {
    const v = JSON.parse(texto) as unknown;
    return typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as ConfigJson) : null;
  } catch {
    return null;
  }
}

/** El texto como lo escribe la app: sangría de 2 espacios y salto final. */
export function aTexto(c: ConfigJson): string {
  return `${JSON.stringify(c, null, 2)}\n`;
}

export function fijarRaiz(c: ConfigJson, raiz: string): ConfigJson {
  return { ...clonar(c), root: raiz };
}

export function comandosDeParte(c: ConfigJson): string[] {
  return c.parts?.commands ?? ['include'];
}

export function fijarComandosDeParte(c: ConfigJson, lista: string[]): ConfigJson {
  const limpia = lista.map((x) => x.trim()).filter(Boolean);
  return { ...clonar(c), parts: { ...clonar(c.parts ?? {}), commands: limpia.length ? [...new Set(limpia)] : ['include'] } };
}

const CLAVE_DE_FLAG: Record<Exclude<UsoDeFlag, 'ninguno'>, 'partial' | 'externalize' | 'figureList'> = {
  partial: 'partial',
  externalize: 'externalize',
  figureList: 'figureList',
};

/** A qué lista de `flags` pertenece una macro hoy (`ninguno` si a ninguna). */
export function usoDeFlag(c: ConfigJson, nombre: string): UsoDeFlag {
  for (const uso of ['partial', 'externalize', 'figureList'] as const) {
    if (c.flags?.[CLAVE_DE_FLAG[uso]]?.includes(nombre)) return uso;
  }
  return 'ninguno';
}

/** Pone la macro en la lista del uso (y la saca de las otras); con `ninguno` la saca de todas. */
export function fijarFlag(c: ConfigJson, nombre: string, uso: UsoDeFlag): ConfigJson {
  const flags = { partial: [] as string[], externalize: [] as string[], figureList: [] as string[], ...clonar(c.flags ?? {}) };
  for (const k of ['partial', 'externalize', 'figureList'] as const) flags[k] = flags[k].filter((x) => x !== nombre);
  if (uso !== 'ninguno') flags[CLAVE_DE_FLAG[uso]].push(nombre);
  return { ...clonar(c), flags };
}

export interface FigurasEditadas {
  modo: 'project' | 'app' | 'off';
  dir: string;
  stampFiles: string[];
}

export function figurasDe(c: ConfigJson): FigurasEditadas {
  const f = c.figureCache;
  if (!f) return { modo: 'off', dir: 'figuras-cache', stampFiles: [] };
  const modo = f.mode === 'app' || f.mode === 'off' ? (f.mode as 'app' | 'off') : 'project';
  return { modo, dir: typeof f.dir === 'string' ? f.dir : 'figuras-cache', stampFiles: Array.isArray(f.stampFiles) ? (f.stampFiles as string[]) : [] };
}

/** `off` saca `figureCache`; los otros modos lo arman o lo actualizan sin perder `onlyInFigureEnv`, `parallelJobs` ni lo que no se conoce. */
export function fijarFiguras(c: ConfigJson, f: FigurasEditadas): ConfigJson {
  const sale = clonar(c);
  if (f.modo === 'off') {
    delete sale.figureCache;
    return sale;
  }
  const previo = sale.figureCache ?? { onlyInFigureEnv: true, parallelJobs: 'auto' };
  const nuevo: Record<string, unknown> = { ...previo, mode: f.modo, dir: f.dir.trim() || 'figuras-cache' };
  const estilos = f.stampFiles.map((x) => x.trim()).filter(Boolean);
  if (estilos.length) nuevo.stampFiles = estilos;
  else delete nuevo.stampFiles;
  sale.figureCache = nuevo;
  return sale;
}

export const ID_DE_PERFIL = /^[A-Za-z0-9_-]+$/;

export function fijarPerfil(c: ConfigJson, id: string, parche: PerfilJson): ConfigJson {
  const sale = clonar(c);
  const previo = sale.profiles?.[id] ?? {};
  const nuevo: PerfilJson = { ...previo, ...parche };
  // Lo que vale lo mismo que el valor por omisión no se escribe (como hace la app al guardar).
  for (const k of ['partial', 'externalize', 'cacheOnly', 'cleanBefore', 'hidden', 'draftImages'] as const) if (nuevo[k] === false) delete nuevo[k];
  if (nuevo.autoCompile === true) delete nuevo.autoCompile;
  if (nuevo.parts === 'all') delete nuevo.parts;
  for (const k of ['partList', 'omit'] as const) if (nuevo[k]?.length === 0) delete nuevo[k];
  if (!nuevo.color) delete nuevo.color;
  if (!nuevo.output) delete nuevo.output;
  if (!nuevo.compileOn) delete nuevo.compileOn;
  if (nuevo.vars && Object.keys(nuevo.vars).length === 0) delete nuevo.vars;
  sale.profiles = { ...(sale.profiles ?? {}), [id]: nuevo };
  return sale;
}

export function quitarPerfil(c: ConfigJson, id: string): ConfigJson {
  const sale = clonar(c);
  if (sale.profiles) delete sale.profiles[id];
  return sale;
}

export interface SalidasEditadas {
  publicar: string;
  exportar: string;
  patron: string;
}

export function salidasDe(c: ConfigJson): SalidasEditadas {
  return { publicar: c.publish?.dir ?? '', exportar: c.export?.dir ?? '', patron: c.export?.pattern ?? '{num:02} - {name}.pdf' };
}

/** Una carpeta vacía quita la sección (no se publica / se exporta a `export/`). Los nombres por perfil de `publish` se conservan. */
export function fijarSalidas(c: ConfigJson, s: SalidasEditadas): ConfigJson {
  const sale = clonar(c);
  if (s.publicar.trim()) sale.publish = { ...(sale.publish ?? {}), dir: s.publicar.trim() };
  else delete sale.publish;
  if (s.exportar.trim() || s.patron.trim() !== '{num:02} - {name}.pdf') sale.export = { ...(sale.export ?? {}), dir: s.exportar.trim() || 'export', pattern: s.patron.trim() || '{num:02} - {name}.pdf' };
  else delete sale.export;
  return sale;
}

export function fijarVariable(c: ConfigJson, nombre: string, v: VariableJson): ConfigJson {
  const sale = clonar(c);
  sale.variables = { ...(sale.variables ?? {}), [nombre]: v };
  return sale;
}

export function quitarVariable(c: ConfigJson, nombre: string): ConfigJson {
  const sale = clonar(c);
  if (sale.variables) delete sale.variables[nombre];
  if (sale.variables && Object.keys(sale.variables).length === 0) delete sale.variables;
  return sale;
}

/** Una macro de TeX: sólo letras (lo mismo que valida el core). */
export const NOMBRE_DE_MACRO = /^[A-Za-z@]+$/;

/** Copia a `c` la sección que salió de la detección (`detectada`): es el «usar lo detectado» de cada parte de la pantalla. */
export function aplicarSeccion(c: ConfigJson, detectada: ConfigJson, seccion: SeccionDeConfig): ConfigJson {
  const sale = clonar(c);
  const d = clonar(detectada);
  const copiar = (clave: string) => {
    if (d[clave] === undefined) delete sale[clave];
    else sale[clave] = d[clave];
  };
  switch (seccion) {
    case 'raiz':
      copiar('root');
      break;
    case 'partes':
      copiar('parts');
      break;
    case 'flags':
      copiar('flags');
      break;
    case 'figuras':
      copiar('figureCache');
      break;
    case 'perfiles':
      copiar('profiles');
      break;
    case 'salidas':
      copiar('publish');
      copiar('export');
      break;
    case 'variables':
      copiar('variables');
      break;
  }
  return sale;
}

/** Cómo se llama el PDF de una parte exportada: `{num}`, `{num:02}` y `{name}` (el mismo cálculo que el core). */
export function nombreDeExportacion(patron: string, num: string, nombre: string): string {
  return patron
    .replace(/\{num:(\d+)\}/g, (_, ancho: string) => num.padStart(Number(ancho), '0'))
    .replace(/\{num\}/g, num)
    .replace(/\{name\}/g, nombre);
}
