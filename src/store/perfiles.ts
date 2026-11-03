// La pantalla de perfiles: cada perfil es una lista de piezas del documento (carátula, índice, el capítulo donde estoy, capítulos
// fijos, bibliografía...) más cómo se compila y adónde va el PDF. Funciones puras sobre el `PerfilJson`, sin React ni backend.
import type { PiezaInfo } from '../backend';
import type { PerfilJson } from './proyectoConfig';

export const COLORES = ['#378ADD', '#1D9E75', '#D85A30', '#7F77DD', '#D4537E', '#BA7517'];

export const nombreCorto = (n: string) => n.split('/').pop() ?? n;

/** Una fila de la lista de piezas, en el orden del libro. */
export type Fila =
  | { tipo: 'pieza'; clave: string; nombre: string }
  | { tipo: 'actual' }
  | { tipo: 'todos' }
  | { tipo: 'capitulo'; parte: string };

/** Lo que va antes de los capítulos, los capítulos y lo que va después. Una pieza sin lugar (no hay capítulos) va antes. */
export function filas(piezas: PiezaInfo[], partes: string[]): Fila[] {
  const pieza = (p: PiezaInfo): Fila => ({ tipo: 'pieza', clave: p.clave, nombre: p.nombre });
  const antes = piezas.filter((p) => p.lugar !== 'despues').map(pieza);
  const despues = piezas.filter((p) => p.lugar === 'despues').map(pieza);
  const caps: Fila[] = partes.length ? [{ tipo: 'actual' }, { tipo: 'todos' }, ...partes.map((parte): Fila => ({ tipo: 'capitulo', parte }))] : [];
  return [...antes, ...caps, ...despues];
}

export interface Seleccion {
  todos: boolean;
  actual: boolean;
  lista: string[];
}

export function seleccionDe(p: PerfilJson): Seleccion {
  const modo = p.parts ?? 'all';
  return { todos: modo === 'all', actual: modo === 'current', lista: p.partList ?? [] };
}

/** De vuelta a `parts`/`partList`: «todos» gana; si no, el actual con los fijos, o sólo los fijos. */
export function aPerfil(s: Seleccion): Pick<PerfilJson, 'parts' | 'partList'> {
  if (s.todos) return { parts: 'all', partList: s.lista };
  if (s.actual) return { parts: 'current', partList: s.lista };
  return { parts: 'list', partList: s.lista };
}

/** ¿Qué capítulos entran, en el orden del documento? `actual` es la parte donde se está trabajando. */
export function capitulosQueEntran(p: PerfilJson, partes: string[], actual: string | null): string[] {
  const s = seleccionDe(p);
  if (s.todos || p.parts === 'selection') return partes;
  return partes.filter((x) => s.lista.includes(x) || (s.actual && x === actual));
}

/** Las piezas (y capítulos) que salen, en orden: lo que dibuja «Lo que sale». */
export function loQueSale(p: PerfilJson, piezas: PiezaInfo[], partes: string[], actual: string | null): { nombre: string; parte?: string; esActual?: boolean }[] {
  const omit = p.omit ?? [];
  const caps = capitulosQueEntran(p, partes, actual);
  const out: { nombre: string; parte?: string; esActual?: boolean }[] = [];
  for (const f of filas(piezas, partes)) {
    if (f.tipo === 'pieza' && !omit.includes(f.clave)) out.push({ nombre: f.nombre });
    if (f.tipo === 'capitulo' && caps.includes(f.parte)) out.push({ nombre: nombreCorto(f.parte), parte: f.parte, esActual: seleccionDe(p).actual && f.parte === actual });
  }
  return out;
}

/** El nombre de un archivo de la salida, como lo arma el core (`config::nombre_de_salida`). */
export function nombreDeSalida(patron: string, n: number, cap: string, nombre: string, perfil: string): string {
  const limpio = (t: string) => t.replace(/[<>:"/\\|?*]/g, '-').trim();
  const sale = patron.replace(/\{(n|cap|num|nombre|name|perfil)(?::0(\d+))?\}/g, (_m, k: string, w?: string) => {
    const v = k === 'n' ? String(n) : k === 'cap' || k === 'num' ? cap : k === 'perfil' ? perfil : nombre;
    const l = limpio(v);
    return w && /^\d+$/.test(l) ? l.padStart(Number(w), '0') : l;
  });
  return /\.pdf$/i.test(sale.trim()) ? sale.trim() : `${sale.trim()}.pdf`;
}

/** Una carpeta absoluta (la del diálogo) relativa a la raíz del proyecto, con `/` y `../` si sale de ella. `null` si está en otra unidad. */
export function carpetaRelativa(raiz: string, carpeta: string): string | null {
  const partes = (x: string) => x.replace(/\\/g, '/').replace(/\/+$/, '').split('/');
  const a = partes(raiz);
  const b = partes(carpeta);
  if (a[0].toLowerCase() !== b[0].toLowerCase()) return null;
  let i = 0;
  while (i < a.length && i < b.length && a[i].toLowerCase() === b[i].toLowerCase()) i++;
  const sube = a.slice(i).map(() => '..');
  const rel = [...sube, ...b.slice(i)].join('/');
  return rel || '.';
}
