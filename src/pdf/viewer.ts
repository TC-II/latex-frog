// Visor PDF sobre PDF.js. Portado del spike S3 (spikes/s3-pdfjs/src/main.ts), técnica B.
//
// Lo que S3 midió y se respeta acá:
// - **Dos visores superpuestos para recargar** (5.7 del plan): el nuevo se arma y se pinta DETRÁS del actual, recibe la
//   misma posición, y recién cuando sus páginas visibles terminaron de pintarse pasa adelante; entonces el viejo se
//   destruye. Medido con el libro (183 y 194 páginas): 0 cuadros en blanco y 0 de deriva en más de 130 recargas, ~220 a
//   230 ms hasta lo visible. `setDocument` sobre el mismo visor deja 4 a 9 cuadros en blanco por recarga y a veces
//   pierde la posición (un salto de 59 páginas).
// - El visor viejo se desarma POR COMPLETO (señal de aborto, limpieza, canvas a 0x0, tarea de carga y documento
//   destruidos): con sólo `doc.destroy()` la memoria crecía ~33 MB por recarga y no se estabilizaba.
// - Cada contenedor es `position: absolute` (o PDF.js falla) y es el elemento con scroll.
// - Los bytes llegan por un comando y PDF.js recibe una COPIA (`slice`): transfiere el buffer a su worker.
import * as pdfjsLib from 'pdfjs-dist';
import { EventBus, PDFLinkService, PDFViewer, RenderingStates } from 'pdfjs-dist/web/pdf_viewer.mjs';
import 'pdfjs-dist/web/pdf_viewer.css';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import type { PdfApi } from './api';
import { palabrasDelTramo } from './metricas';
import { clave } from '../sync/texto';
import type { PalabraPdf, Rect } from '../sync/buscar';

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

export interface PdfEstado {
  pagina: number;
  paginas: number;
  escala: number;
  modo: 'ancho' | 'manual';
}

interface Posicion {
  pagina: number;
  offset: number;
  escala: number;
}

/** Medición de una recarga (la misma que hizo S3): sirve para el smoke de aceptación. */
export interface MedicionRecarga {
  ms: number;
  cuadros: number;
  cuadrosEnBlanco: number;
  rachaMaxBlancos: number;
  derivaPagina: number;
  derivaPx: number;
}

interface Visor {
  cont: HTMLDivElement;
  viewer: any;
  bus: any;
  link: any;
  doc: any;
  task: any;
  ac: AbortController;
}

const ESCALA_MIN = 0.25;
const ESCALA_MAX = 5;
/** Si el visor nuevo no termina de pintar lo visible en este tiempo, se hace el cambio igual. */
const ESPERA_MAX_MS = 20_000;

const frame = () => new Promise<number>((r) => requestAnimationFrame(r));

const visiblesListos = (v: Visor): boolean => {
  const vistas = v.viewer?._getVisiblePages?.().views as { view: any }[] | undefined;
  return !!vistas && vistas.length > 0 && vistas.every((x) => x.view.renderingState === RenderingStates.FINISHED);
};

/**
 * Nitidez con escalas de pantalla fraccionarias (125 %, 150 %). PDF.js pinta cada página en un canvas con exactamente
 * `devicePixelRatio` píxeles por px de CSS, pero el borde de la página (9 px de CSS = 11,25 px reales a 125 %) y el centrado
 * dejan el canvas en una posición fraccionaria de la pantalla, y Chromium lo copia con interpolación bilineal: el texto
 * queda borroso aunque la resolución sea la correcta. Medido en Chromium a 125 %: el canvas se pintaba corrido 0,25 px en x
 * y 0,5 px en y y la captura difería del bitmap; con `image-rendering: pixelated` la captura es el bitmap exacto, píxel a píxel.
 * Alinear el canvas desde el layout (`translate` o `left`) no sirve: Chromium no lo pinta donde dice `getBoundingClientRect`.
 * Sólo se pone cuando el canvas está 1:1 con la pantalla: mientras PDF.js estira el canvas viejo durante un zoom (o a zoom muy
 * alto, cuando pinta una versión reducida debajo del detalle), el vecino más cercano se vería en escalones.
 */
function nitidez(div: HTMLElement): void {
  const dpr = window.devicePixelRatio || 1;
  div.querySelectorAll<HTMLCanvasElement>('.canvasWrapper canvas').forEach((c) => {
    const ancho = c.getBoundingClientRect().width * dpr;
    c.style.imageRendering = ancho > 0 && Math.abs(c.width - ancho) <= 1 ? 'pixelated' : '';
  });
}

async function esperarVisibles(v: Visor, limiteMs: number): Promise<void> {
  const t0 = performance.now();
  while (!visiblesListos(v) && performance.now() - t0 < limiteMs) await frame();
}

export class PdfViewerHost implements PdfApi {
  private actual: Visor | null = null;
  private modo: 'ancho' | 'manual' = 'ancho';
  private readonly oyentes = new Set<(e: PdfEstado) => void>();
  private readonly observador: ResizeObserver;
  private temporizador: ReturnType<typeof setTimeout> | null = null;
  /** Las palabras de cada página, por documento: se arman una vez y se tiran con el documento. */
  private readonly palabras = new WeakMap<object, Map<number, PalabraPdf[]>>();
  /** El contexto de un canvas que no se muestra: sólo sirve para medir texto (`palabrasDePagina`). */
  private ctxMedida: CanvasRenderingContext2D | null = null;
  /** Las cargas se encolan: dos recargas seguidas no pueden pisarse a mitad de armar o de desarmar. */
  private cola: Promise<unknown> = Promise.resolve();

  /** `area` es el elemento (posicionado) donde se arman los contenedores de cada visor. */
  constructor(private readonly area: HTMLDivElement) {
    // En modo "ancho de página" el zoom sigue al tamaño del panel.
    this.observador = new ResizeObserver(() => {
      if (this.modo !== 'ancho' || !this.actual) return;
      if (this.temporizador) clearTimeout(this.temporizador);
      this.temporizador = setTimeout(() => {
        if (this.actual && this.modo === 'ancho') this.actual.viewer.currentScaleValue = 'page-width';
      }, 120);
    });
    this.observador.observe(area);
  }

  private get viewer(): any {
    return this.actual?.viewer ?? null;
  }

  get tieneDocumento(): boolean {
    return this.actual !== null;
  }

  get paginas(): number {
    return this.viewer?.pagesCount ?? 0;
  }

  estado(): PdfEstado {
    return {
      pagina: this.viewer?.currentPageNumber ?? 0,
      paginas: this.paginas,
      escala: this.viewer?.currentScale ?? 1,
      modo: this.modo,
    };
  }

  suscribir(cb: (e: PdfEstado) => void): () => void {
    this.oyentes.add(cb);
    return () => {
      this.oyentes.delete(cb);
    };
  }

  private emitir(): void {
    const e = this.estado();
    this.oyentes.forEach((cb) => cb(e));
  }

  // --- armar, mostrar y desarmar un visor -----------------------------------------------------------------------------

  private crear(z: number): Visor {
    const cont = document.createElement('div');
    cont.className = 'pdf-contenedor';
    cont.style.zIndex = String(z);
    const v = document.createElement('div');
    v.className = 'pdfViewer';
    cont.appendChild(v);
    this.area.appendChild(cont);
    const bus = new EventBus();
    const link = new PDFLinkService({ eventBus: bus });
    // `abortSignal` está en el código de PDFViewer (lo usa para soltar el ResizeObserver y los listeners) pero no en sus
    // tipos de la 6.4.299: de ahí el `as any`. Es lo que estabiliza la memoria (S3).
    const ac = new AbortController();
    const viewer = new PDFViewer({ container: cont, viewer: v, eventBus: bus, linkService: link, abortSignal: ac.signal } as any);
    link.setViewer(viewer);
    bus.on('pagerendered', (e: any) => {
      if (e?.source?.div) nitidez(e.source.div);
    });
    // Al cambiar el zoom PDF.js estira los canvas que ya tiene hasta volver a pintarlos: sin interpolación se verían en escalones.
    bus.on('scalechanging', () => cont.querySelectorAll<HTMLCanvasElement>('canvas').forEach((c) => (c.style.imageRendering = '')));
    return { cont, viewer, bus, link, doc: null, task: null, ac };
  }

  private async abrir(v: Visor, bytes: Uint8Array): Promise<void> {
    const task = pdfjsLib.getDocument({ data: bytes.slice() });
    v.task = task;
    const doc = await task.promise;
    v.doc = doc;
    const listo = new Promise<void>((r) => v.bus.on('pagesinit', () => r(), { once: true }));
    v.viewer.setDocument(doc);
    v.link.setDocument(doc);
    await listo;
  }

  /** Desarma un visor entero: ver el comentario de arriba (sin esto la memoria crece en cada recarga). */
  private async liberar(v: Visor): Promise<void> {
    v.ac.abort();
    try {
      v.viewer.cleanup?.();
    } catch {
      /* sin documento */
    }
    v.cont.querySelectorAll('canvas').forEach((c) => {
      c.width = 0;
      c.height = 0;
    });
    try {
      v.viewer.setDocument(null);
    } catch {
      /* ya estaba vacío */
    }
    try {
      v.link.setDocument(null);
    } catch {
      /* idem */
    }
    await v.task?.destroy?.();
    await v.doc?.destroy?.();
    v.cont.remove();
  }

  private posicionDe(v: Visor): Posicion | null {
    const pagina = v.viewer.currentPageNumber as number;
    const pv = v.viewer.getPageView(pagina - 1);
    if (!pv) return null;
    return { pagina, offset: v.cont.scrollTop - pv.div.offsetTop, escala: v.viewer.currentScale };
  }

  private restaurar(v: Visor, previa: Posicion | null): void {
    if (!previa) {
      this.modo = 'ancho';
      v.viewer.currentScaleValue = 'page-width';
      return;
    }
    if (this.modo === 'manual') v.viewer.currentScale = previa.escala;
    else v.viewer.currentScaleValue = 'page-width';
    const pv = v.viewer.getPageView(Math.min(previa.pagina, v.viewer.pagesCount) - 1);
    if (pv) v.cont.scrollTop = pv.div.offsetTop + previa.offset;
  }

  /** `etiqueta` identifica qué PDF es (el store pasa `<ruta>#<época>`): queda en `etiqueta` al terminar la carga. */
  cargar(bytes: Uint8Array, etiqueta: string | null = null): Promise<void> {
    const trabajo = this.cola.then(() => this.cargarAhora(bytes, etiqueta));
    this.cola = trabajo.catch(() => undefined);
    return trabajo;
  }

  private async cargarAhora(bytes: Uint8Array, etiqueta: string | null): Promise<void> {
    const viejo = this.actual;
    const previa = viejo ? this.posicionDe(viejo) : null;
    // El primero va al frente; los siguientes se arman detrás del que se está mostrando.
    const nuevo = this.crear(viejo ? 1 : 2);
    try {
      await this.abrir(nuevo, bytes);
    } catch (e) {
      await this.liberar(nuevo);
      throw e;
    }
    this.restaurar(nuevo, previa);
    if (viejo) {
      await esperarVisibles(nuevo, ESPERA_MAX_MS);
      // Recién ahora el nuevo pasa adelante.
      nuevo.cont.style.zIndex = '2';
      viejo.cont.style.zIndex = '1';
      this.actual = nuevo;
      await frame();
      await frame();
      await this.liberar(viejo);
    } else {
      this.actual = nuevo;
    }
    nuevo.bus.on('pagechanging', () => this.emitir());
    nuevo.bus.on('scalechanging', () => this.emitir());
    this.etiqueta = etiqueta;
    this.emitir();
  }

  /** Recarga midiendo lo mismo que S3: tiempo hasta lo visible, cuadros en blanco del visor mostrado y deriva. */
  async cargarMedido(bytes: Uint8Array): Promise<MedicionRecarga> {
    const antes = this.actual ? this.posicionDe(this.actual) : null;
    const r = { cuadros: 0, blancos: 0, racha: 0, maxRacha: 0 };
    let activo = true;
    void (async () => {
      while (activo) {
        await frame();
        r.cuadros++;
        // El visor que se está mostrando: el actual hasta el cambio.
        if (this.actual && !visiblesListos(this.actual)) {
          r.blancos++;
          r.racha++;
          r.maxRacha = Math.max(r.maxRacha, r.racha);
        } else r.racha = 0;
      }
    })();
    const t0 = performance.now();
    await this.cargar(bytes);
    const ms = performance.now() - t0;
    activo = false;
    const despues = this.actual ? this.posicionDe(this.actual) : null;
    return {
      ms: Math.round(ms),
      cuadros: r.cuadros,
      cuadrosEnBlanco: r.blancos,
      rachaMaxBlancos: r.maxRacha,
      derivaPagina: despues && antes ? despues.pagina - antes.pagina : 0,
      derivaPx: despues && antes ? Math.round((despues.offset - antes.offset) * 10) / 10 : 0,
    };
  }

  async destruir(): Promise<void> {
    this.observador.disconnect();
    if (this.temporizador) clearTimeout(this.temporizador);
    await this.cola;
    const v = this.actual;
    this.actual = null;
    if (v) await this.liberar(v);
    this.oyentes.clear();
  }

  // --- zoom y navegación ------------------------------------------------------------------------------------------

  setEscala(escala: number): void {
    if (!this.viewer) return;
    this.modo = 'manual';
    this.viewer.currentScale = Math.min(ESCALA_MAX, Math.max(ESCALA_MIN, escala));
    this.emitir();
  }

  zoomMas(): void {
    if (this.viewer) this.setEscala(this.viewer.currentScale * 1.1);
  }

  zoomMenos(): void {
    if (this.viewer) this.setEscala(this.viewer.currentScale / 1.1);
  }

  ajustarAncho(): void {
    if (!this.viewer) return;
    this.modo = 'ancho';
    this.viewer.currentScaleValue = 'page-width';
    this.emitir();
  }

  irA(pagina: number): void {
    if (!this.viewer) return;
    this.viewer.currentPageNumber = Math.min(this.paginas, Math.max(1, Math.round(pagina)));
  }

  siguiente(): void {
    this.irA(this.estado().pagina + 1);
  }

  anterior(): void {
    this.irA(this.estado().pagina - 1);
  }

  // --- SyncTeX: palabras, resaltado y desplazamiento -------------------------------------------------------------------

  /**
   * Las palabras de la capa de texto de una página, con su rectángulo en bp desde arriba a la izquierda (el espacio de
   * SyncTeX). PDF.js entrega una cadena por tramo de texto con su posición y su ancho; las palabras se sacan repartiendo
   * el ancho en proporción a los caracteres, que alcanza para ubicar un clic y para resaltar.
   */
  async palabrasDePagina(n: number): Promise<PalabraPdf[]> {
    const doc = this.actual?.doc;
    if (!doc || n < 1 || n > doc.numPages) return [];
    let porPagina = this.palabras.get(doc);
    if (!porPagina) this.palabras.set(doc, (porPagina = new Map()));
    const hecho = porPagina.get(n);
    if (hecho) return hecho;
    const pagina = await doc.getPage(n);
    const [x0, y0, , y1] = pagina.view as number[];
    const alto = y1 - y0;
    const contenido = await pagina.getTextContent();
    // Las fuentes de la página se cargan al armar su lista de operaciones (lo mismo que hace dibujarla): sin ellas el canvas mide con una
    // fuente cualquiera. Si falla, se mide igual (con la de reserva sigue siendo mejor que repartir por cantidad de caracteres).
    try {
      await pagina.getOperatorList();
    } catch {
      // se sigue sin las fuentes
    }
    const ctx = (this.ctxMedida ??= document.createElement('canvas').getContext('2d'));
    const out: PalabraPdf[] = [];
    for (const it of contenido.items as any[]) {
      const str: string | undefined = it.str;
      if (!str || !str.trim() || !it.transform) continue;
      const [, , c, d, e, f] = it.transform as number[];
      const tam = Math.hypot(c, d) || it.height || 10; // tamaño de la letra
      const ancho: number = it.width || tam * 0.5 * str.length;
      const baseX = e - x0;
      const baseY = alto - (f - y0); // la línea de base, medida desde arriba
      // Dónde cae cada palabra: midiendo el texto con la fuente del tramo, como la capa de texto de PDF.js (ver `metricas.ts`).
      const familia: string | undefined = contenido.styles?.[it.fontName]?.fontFamily;
      let medir: ((t: string) => number) | null = null;
      if (ctx && familia) {
        ctx.font = `${tam}px ${familia}`;
        medir = (t) => ctx.measureText(t).width;
      }
      for (const p of palabrasDelTramo(str, ancho, medir)) {
        const k = clave(p.texto);
        if (!k) continue;
        out.push({ texto: p.texto, clave: k, page: n, x: baseX + p.x, y: baseY - tam * 0.78, w: p.w, h: tam });
      }
    }
    porPagina.set(n, out);
    return out;
  }

  private vistaDePagina(n: number): { pv: any; escala: number } | null {
    const pv = this.actual?.viewer.getPageView(n - 1);
    return pv ? { pv, escala: pv.viewport.scale } : null;
  }

  /** Resalta rectángulos (bp) de una página con una capa que se desvanece sola (6.2 del plan: ~1,5 s). */
  resaltar(pagina: number, rects: Rect[], ms = 1500): void {
    const v = this.vistaDePagina(pagina);
    if (!v) return;
    this.area.querySelectorAll('.lv-resaltados').forEach((e) => e.remove());
    const capa = document.createElement('div');
    capa.className = 'lv-resaltados';
    for (const r of rects.filter((x) => x.page === pagina)) {
      const el = document.createElement('div');
      el.className = 'lv-resaltado';
      Object.assign(el.style, { left: `${r.x * v.escala}px`, top: `${r.y * v.escala}px`, width: `${Math.max(r.w, 2) * v.escala}px`, height: `${r.h * v.escala}px`, animationDuration: `${ms}ms` });
      capa.appendChild(el);
    }
    v.pv.div.appendChild(capa);
    setTimeout(() => capa.remove(), ms + 100);
  }

  /** La marca fija de «Seguir»: dónde está el cursor del editor en el PDF. No se desvanece; la siguiente la reemplaza. `null` la saca. */
  marcar(pagina: number | null, rects: Rect[] = []): void {
    this.area.querySelectorAll('.lv-cursor-pdf').forEach((e) => e.remove());
    if (pagina === null) return;
    const v = this.vistaDePagina(pagina);
    if (!v) return;
    const capa = document.createElement('div');
    capa.className = 'lv-resaltados lv-cursor-pdf';
    for (const r of rects.filter((x) => x.page === pagina)) {
      const el = document.createElement('div');
      el.className = 'lv-marca';
      Object.assign(el.style, { left: `${r.x * v.escala}px`, top: `${r.y * v.escala}px`, width: `${Math.max(r.w, 2) * v.escala}px`, height: `${r.h * v.escala}px` });
      capa.appendChild(el);
    }
    v.pv.div.appendChild(capa);
  }

  esVisible(pagina: number, rect: Rect): boolean {
    const v = this.vistaDePagina(pagina);
    const cont = this.actual?.cont;
    if (!v || !cont) return false;
    const arriba = v.pv.div.offsetTop + rect.y * v.escala;
    const izq = v.pv.div.offsetLeft + rect.x * v.escala;
    return (
      arriba >= cont.scrollTop + 16 &&
      arriba + rect.h * v.escala <= cont.scrollTop + cont.clientHeight - 16 &&
      izq >= cont.scrollLeft &&
      izq + rect.w * v.escala <= cont.scrollLeft + cont.clientWidth
    );
  }

  /** Desplaza el PDF sólo si el rectángulo no estaba a la vista, y lo deja a un tercio de la altura. */
  mostrar(pagina: number, rect: Rect): void {
    const v = this.vistaDePagina(pagina);
    const cont = this.actual?.cont;
    if (!v || !cont || this.esVisible(pagina, rect)) return;
    // Primero se le dice a PDF.js cuál es la página actual: si sólo se movía `scrollTop`, un cambio de tamaño del panel (que
    // reaplica el zoom y restaura "la página actual") devolvía el visor a donde estaba antes del salto (medido en el smoke de F4).
    if (this.actual?.viewer.currentPageNumber !== pagina) this.actual!.viewer.currentPageNumber = pagina;
    cont.scrollTop = Math.max(0, v.pv.div.offsetTop + rect.y * v.escala - cont.clientHeight / 3);
    const izq = v.pv.div.offsetLeft + rect.x * v.escala;
    if (izq < cont.scrollLeft || izq + rect.w * v.escala > cont.scrollLeft + cont.clientWidth) cont.scrollLeft = Math.max(0, izq - cont.clientWidth / 3);
  }

  /** La página y el punto (bp) de unas coordenadas de pantalla que caen sobre el elemento `pagina` (un `.page`). */
  private aPunto(pagina: HTMLElement | null, cx: number, cy: number): { page: number; x: number; y: number } | null {
    if (!pagina) return null;
    const n = Number(pagina.dataset.pageNumber);
    const v = this.vistaDePagina(n);
    if (!v) return null;
    const r = pagina.getBoundingClientRect();
    return { page: n, x: (cx - (r.left + pagina.clientLeft)) / v.escala, y: (cy - (r.top + pagina.clientTop)) / v.escala };
  }

  /** La página y el punto (bp) de un evento del mouse sobre el visor. `null` si no cayó sobre una página. */
  puntoDeEvento(ev: MouseEvent): { page: number; x: number; y: number } | null {
    return this.aPunto((ev.target as HTMLElement | null)?.closest?.('.page') as HTMLElement | null, ev.clientX, ev.clientY);
  }

  /**
   * El texto seleccionado en el PDF (la capa de texto de PDF.js): su comienzo y su final, cada uno en la página y el punto (bp) donde
   * están. Se toman a media altura del primer y del último renglón de la selección, un poco hacia adentro, para caer sobre una letra.
   * `null` si no hay selección, no es del visor del documento o no se pudo ubicar.
   */
  puntoDeSeleccion(): { inicio: { page: number; x: number; y: number }; fin: { page: number; x: number; y: number } } | null {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || sel.rangeCount === 0) return null;
    const rango = sel.getRangeAt(0);
    if (!this.area.contains(rango.commonAncestorContainer)) return null;
    const cajas = [...rango.getClientRects()].filter((r) => r.width > 0 && r.height > 0);
    if (cajas.length === 0) return null;
    const primera = cajas[0];
    const ultima = cajas[cajas.length - 1];
    // La página de cada punta sale del nodo donde la selección empieza y termina (no de `elementFromPoint`: no depende de que esté a la vista).
    const paginaDe = (n: Node) => (n instanceof Element ? n : n.parentElement)?.closest('.page') as HTMLElement | null;
    const inicio = this.aPunto(paginaDe(rango.startContainer), primera.left + Math.min(2, primera.width / 2), primera.top + primera.height / 2);
    const fin = this.aPunto(paginaDe(rango.endContainer), ultima.right - Math.min(2, ultima.width / 2), ultima.top + ultima.height / 2);
    return inicio && fin ? { inicio, fin } : null;
  }

  centroVisible(): { page: number; x: number; y: number } | null {
    const cont = this.actual?.cont;
    if (!cont) return null;
    const r = cont.getBoundingClientRect();
    const centro = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    const pagina = (centro as HTMLElement | null)?.closest?.('.page') as HTMLElement | null;
    if (!pagina) return null;
    const n = Number(pagina.dataset.pageNumber);
    const v = this.vistaDePagina(n);
    if (!v) return null;
    const rp = pagina.getBoundingClientRect();
    return { page: n, x: (r.left + r.width / 2 - (rp.left + pagina.clientLeft)) / v.escala, y: (r.top + r.height / 2 - (rp.top + pagina.clientTop)) / v.escala };
  }

  // --- para los smokes de aceptación -----------------------------------------------------------------------------------

  /** Las páginas que se ven están pintadas del todo (no placeholders ni a medio render). */
  paginasVisiblesListas(): boolean {
    return !!this.actual && visiblesListos(this.actual);
  }

  /** La etiqueta del último PDF cargado del todo (con el swap hecho y el visor viejo ya desarmado). */
  etiqueta: string | null = null;

  /** Cuántos contenedores de visor hay armados ahora mismo: tiene que ser 1 fuera de una recarga. */
  get visoresArmados(): number {
    return this.area.querySelectorAll('.pdf-contenedor').length;
  }

  /** Primera página (1..N) cuyo texto contiene `texto`, o null. */
  async paginaConTexto(texto: string): Promise<number | null> {
    const visor = this.actual;
    const doc = visor?.doc;
    if (!doc) return null;
    const buscar = async (): Promise<number | null> => {
      for (let i = 1; i <= doc.numPages; i++) {
        // Si una recarga cambió de visor a mitad de la búsqueda, el documento viejo se destruye y `getPage` no vuelve.
        if (this.actual !== visor) return null;
        const pagina = await doc.getPage(i);
        const contenido = await pagina.getTextContent();
        if (contenido.items.map((it: any) => it.str ?? '').join(' ').includes(texto)) return i;
      }
      return null;
    };
    return Promise.race([buscar().catch(() => null), new Promise<null>((r) => setTimeout(() => r(null), 30_000))]);
  }
}

/** El visor que está en pantalla, para que el smoke pueda mirarlo sin pasar por React. */
export const pdfActual: { host: PdfViewerHost | null } = { host: null };
