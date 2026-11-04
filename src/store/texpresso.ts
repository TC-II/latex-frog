// La vista previa en vivo con TeXpresso (en WSL), del lado de la interfaz: el interruptor de la barra, su estado y lo que se le manda
// mientras se edita (el texto del buffer y la línea del cursor). El core lanza y vigila el proceso; acá no se lanza nada (3.2 del
// plan: todo pasa por `Backend`). Apagado, nada de esto manda nada: la app compila con latexmk como siempre.
import { create } from 'zustand';
import { getBackend, type DisponibilidadTexpresso, type EventoTexpresso, type VistaTexpresso } from '../backend';
import { getDoc } from '../editor/docs';

/** Los archivos que TeXpresso lee como texto: los demás (imágenes, PDF) no se le mandan. */
export const esFuenteTex = (rel: string): boolean => /\.(tex|sty|cls|ltx|def|cfg|bib|bbx|cbx)$/i.test(rel);

export interface Boton {
  texto: string;
  /** Para el CSS: `apagado`, `iniciando`, `listo`, `caido`, `no_disponible`. */
  clase: string;
  titulo: string;
  deshabilitado: boolean;
  /** Lo que hace el clic: prender o apagar. */
  prender: boolean;
}

const ms = (n: number | null) => (n === null ? '' : n < 1000 ? `${n} ms` : `${(n / 1000).toFixed(1)} s`);

/** Lo que muestra el botón de la barra, según la fase y si se puede prender. Pura. */
export function boton(vista: VistaTexpresso | null, disp: DisponibilidadTexpresso | null, pidiendo: boolean): Boton {
  const base = 'Vista previa en vivo con TeXpresso (en WSL), en su propia ventana. Lo que se compila con ▶ y lo que se entrega sigue saliendo de latexmk.';
  if (pidiendo) return { texto: '⚡ TeXpresso: buscando…', clase: 'iniciando', titulo: 'Mirando adentro de la distro de WSL', deshabilitado: true, prender: false };
  const fase = vista?.fase ?? 'apagado';
  if (fase === 'iniciando') return { texto: '⚡ TeXpresso: iniciando…', clase: 'iniciando', titulo: `${base}\nClic para apagarla.`, deshabilitado: false, prender: false };
  if (fase === 'listo') {
    const extra = vista?.errores ? ` · ${vista.errores} error${vista.errores === 1 ? '' : 'es'}` : vista?.ultimoMs !== null && vista?.ultimoMs !== undefined ? ` · ${ms(vista.ultimoMs)}` : '';
    const motivo = vista?.errores && vista.motivo ? `\nPrimer error: ${vista.motivo}` : '';
    return { texto: `⚡ TeXpresso: listo${extra}`, clase: vista?.errores ? 'listo con-errores' : 'listo', titulo: `${base}${motivo}\nClic para apagarla.`, deshabilitado: false, prender: false };
  }
  if (fase === 'caido') {
    const reintenta = vista?.motivo?.includes('reiniciando') ?? false;
    return {
      texto: reintenta ? '⚡ TeXpresso: caído, reiniciando…' : '⚡ TeXpresso: caído',
      clase: 'caido',
      titulo: `${vista?.motivo ?? 'TeXpresso se cayó.'}\nClic para apagarla.`,
      deshabilitado: false,
      prender: false,
    };
  }
  // Apagado o no disponible: el motivo, si se sabe que no se puede.
  if (disp && !disp.disponible) {
    return { texto: '⚡ TeXpresso: no disponible', clase: 'no_disponible', titulo: `No se puede usar TeXpresso: ${disp.motivo ?? 'falta algo'}.`, deshabilitado: true, prender: true };
  }
  if (fase === 'no_disponible') {
    return { texto: '⚡ TeXpresso: no disponible', clase: 'no_disponible', titulo: `No se puede usar TeXpresso: ${vista?.motivo ?? 'falta algo'}.`, deshabilitado: true, prender: true };
  }
  return { texto: '⚡ TeXpresso', clase: 'apagado', titulo: `${base}\nClic para prenderla.`, deshabilitado: !disp, prender: true };
}

/** La última página que TeXpresso mandó como imagen (pestaña «En vivo»). */
export interface ImagenVivo {
  /** Desde 0. */
  pagina: number;
  paginas: number;
  /** Tamaño natural de la imagen, en píxeles: los clics se mandan en estas coordenadas. */
  ancho: number;
  alto: number;
  /** `data:image/png;base64,...` */
  src: string;
  /** Cuántas imágenes llegaron desde que se prendió (el smoke espera a que suba). */
  n: number;
}

interface TexpressoState {
  vista: VistaTexpresso | null;
  disp: DisponibilidadTexpresso | null;
  /** Prendiendo: se está mirando adentro de la distro. */
  pidiendo: boolean;
  /** Los tiempos de los últimos cambios mostrados (para el smoke y el título). */
  tiempos: number[];
  imagen: ImagenVivo | null;
  cargar(): Promise<void>;
  alternar(): Promise<void>;
  activar(on: boolean): Promise<void>;
}

export const useTexpresso = create<TexpressoState>()((set, get) => ({
  vista: null,
  disp: null,
  pidiendo: false,
  tiempos: [],
  imagen: null,
  async cargar() {
    set({ vista: null, disp: null, pidiendo: false, tiempos: [], imagen: null });
    try {
      const e = await getBackend().texpressoEstado();
      set({ vista: e.vista, disp: e.disponibilidad });
    } catch {
      // Sin proyecto o sin el comando (un backend de prueba): el botón queda deshabilitado.
    }
  },
  async alternar() {
    const b = boton(get().vista, get().disp, get().pidiendo);
    if (b.deshabilitado) return;
    await get().activar(b.prender);
  },
  async activar(on) {
    if (on) set({ pidiendo: true });
    try {
      const e = await getBackend().texpressoActivar(on);
      set({ vista: e.vista, disp: e.disponibilidad });
      if (on && e.vista.fase !== 'no_disponible') {
        // Lo que ya está abierto: TeXpresso arranca leyendo el disco y hay que mandarle lo que el editor tiene sin guardar.
        alAbrirLaVista?.();
      }
    } finally {
      set({ pidiendo: false });
    }
  },
}));

let alAbrirLaVista: (() => void) | null = null;
/** El store de la app manda la pestaña activa (texto y cursor) apenas TeXpresso se prende. */
export function setAlPrender(f: () => void): void {
  alAbrirLaVista = f;
}

let irAlCodigo: (rel: string, line: number) => void = () => {};
/** Clic en la ventana de TeXpresso: el store de la app abre el archivo en esa línea. */
export function setIrAlCodigo(f: (rel: string, line: number) => void): void {
  irAlCodigo = f;
}

let desuscribir: (() => void) | null = null;

/** Se suscribe a los eventos del core una sola vez por proyecto abierto. */
export async function iniciarTexpresso(): Promise<void> {
  desuscribir?.();
  desuscribir = null;
  await useTexpresso.getState().cargar();
  try {
    desuscribir = await getBackend().onTexpresso((e: EventoTexpresso) => {
      if (e.type === 'estado') {
        // Apagado, no queda la última página dibujada: la pestaña «En vivo» volvería a mostrar algo viejo al prenderlo.
        useTexpresso.setState((s) => ({ vista: e.vista, imagen: e.vista.fase === 'apagado' || e.vista.fase === 'no_disponible' ? null : s.imagen }));
      } else if (e.type === 'actualizado') useTexpresso.setState((s) => ({ tiempos: [...s.tiempos, e.ms].slice(-200) }));
      else if (e.type === 'sync') irAlCodigo(e.rel, e.line);
      else if (e.type === 'pagina') {
        useTexpresso.setState((s) => ({
          imagen: { pagina: e.pagina, paginas: e.paginas, ancho: e.ancho, alto: e.alto, src: `data:image/png;base64,${e.png}`, n: (s.imagen?.n ?? 0) + 1 },
        }));
      }
    });
  } catch {
    // Un backend sin eventos de TeXpresso.
  }
}

/** Si el interruptor está prendido (iniciando, listo o caído y reintentando). */
export const estaPrendido = (v: VistaTexpresso | null): boolean => v?.fase === 'listo' || v?.fase === 'iniciando' || v?.fase === 'caido';

const prendido = (): boolean => estaPrendido(useTexpresso.getState().vista);

// Con debounce: cada tecla no tiene por qué cruzar a WSL (TeXpresso tarda 6 a 85 ms por cambio y los cambios seguidos se pisan).
export const DEBOUNCE_TEXTO_MS = 60;
export const DEBOUNCE_CURSOR_MS = 150;
const temporizadores = new Map<string, ReturnType<typeof setTimeout>>();
let temporizadorCursor: ReturnType<typeof setTimeout> | null = null;

/** El documento cambió en el editor: se le manda el texto a TeXpresso (si está prendido). */
export function texpressoAlEditar(rel: string): void {
  if (!prendido() || !esFuenteTex(rel)) return;
  const t = temporizadores.get(rel);
  if (t) clearTimeout(t);
  temporizadores.set(
    rel,
    setTimeout(() => {
      temporizadores.delete(rel);
      const texto = textoParaTexpresso(rel);
      if (texto !== null) void getBackend().texpressoBuffer(rel, texto).catch(() => {});
    }, DEBOUNCE_TEXTO_MS),
  );
}

/**
 * El texto que se le manda a TeXpresso: el del editor con el EOL y el BOM que tiene el archivo en disco. El editor normaliza a LF y
 * TeXpresso compara byte a byte contra lo que leyó del disco: con un archivo CRLF (el libro de Javier lo es) el primer cambio difería en
 * el primer salto de línea y forzaba una vuelta atrás hasta el principio del capítulo (se vio: de 11 páginas a 0).
 */
export function textoParaTexpresso(rel: string): string | null {
  const e = getDoc(rel);
  if (!e) return null;
  const t = e.state.doc.toString();
  const conEol = e.eol === 'crlf' ? t.replace(/\n/g, '\r\n') : e.eol === 'cr' ? t.replace(/\n/g, '\r') : t;
  return e.bom ? '﻿' + conEol : conEol;
}

/** La línea (1 en adelante) del cursor del documento abierto. */
export function lineaDelCursor(rel: string): number | null {
  const e = getDoc(rel);
  if (!e) return null;
  return e.state.doc.lineAt(e.state.selection.main.head).number;
}

/** El cursor se movió: TeXpresso muestra esa página (y compila hasta ahí: es perezoso). */
export function texpressoAlMoverCursor(rel: string): void {
  if (!prendido() || !esFuenteTex(rel)) return;
  if (temporizadorCursor) clearTimeout(temporizadorCursor);
  temporizadorCursor = setTimeout(() => {
    temporizadorCursor = null;
    const l = lineaDelCursor(rel);
    if (l !== null) void getBackend().texpressoPosicion(rel, l).catch(() => {});
  }, DEBOUNCE_CURSOR_MS);
}

/** La pestaña se cerró: TeXpresso vuelve a lo que hay en disco. */
export function texpressoAlCerrar(rel: string): void {
  if (!prendido()) return;
  const t = temporizadores.get(rel);
  if (t) clearTimeout(t);
  temporizadores.delete(rel);
  void getBackend().texpressoCerrar(rel).catch(() => {});
}

/** Manda ya, sin debounce, el texto y el cursor de `rel` (al prender, o al cambiar de pestaña). */
export function texpressoMandarYa(rel: string): void {
  if (!prendido() || !esFuenteTex(rel)) return;
  const texto = textoParaTexpresso(rel);
  if (texto !== null) void getBackend().texpressoBuffer(rel, texto).catch(() => {});
  const l = lineaDelCursor(rel);
  if (l !== null) void getBackend().texpressoPosicion(rel, l).catch(() => {});
}
