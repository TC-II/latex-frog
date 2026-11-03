// Lo que el store necesita del visor del PDF para SyncTeX. Es una interfaz mínima, separada de `viewer.ts`, porque el
// store no puede importar `pdfjs-dist` (los tests de Vitest no lo cargan): el visor real la implementa y se registra acá.
import type { PalabraPdf, Rect } from '../sync/buscar';

export interface PdfApi {
  /** La etiqueta del último PDF cargado del todo (`<ruta>#<época>`): sirve para esperar a que una recarga termine. */
  readonly etiqueta: string | null;
  /** Las palabras de la capa de texto de la página `n` (1..N), con su rectángulo en bp desde arriba a la izquierda. */
  palabrasDePagina(n: number): Promise<PalabraPdf[]>;
  /** Resalta rectángulos de una página; el resaltado se desvanece solo. */
  resaltar(pagina: number, rects: Rect[], ms?: number): void;
  /** La marca fija de «Seguir» (dónde está el cursor); `null` la saca. */
  marcar?(pagina: number | null, rects?: Rect[]): void;
  /** Desplaza el PDF para que el rectángulo se vea, sólo si no estaba a la vista (lo deja a un tercio de la altura). */
  mostrar(pagina: number, rect: Rect): void;
  /** Si el rectángulo ya se ve en el visor. */
  esVisible(pagina: number, rect: Rect): boolean;
  /** La página y el punto (en bp) que están en el centro de lo que se ve. */
  centroVisible(): { page: number; x: number; y: number } | null;
  /**
   * El texto que la persona seleccionó en el PDF: dónde empieza y dónde termina (página y punto en bp), o `null` si no hay selección.
   * Es de lo que parte «ir al código» desde el botón de la barra separadora; sin selección se usa el centro de lo que se ve.
   */
  puntoDeSeleccion?(): { inicio: PuntoPdf; fin: PuntoPdf } | null;
}

export interface PuntoPdf {
  page: number;
  x: number;
  y: number;
}

export const pdfApi: { host: PdfApi | null } = { host: null };
