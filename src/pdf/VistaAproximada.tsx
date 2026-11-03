// Las pestañas dibujadas al instante, sin TeX (`vista/aproximada.ts`): «Al instante» es el archivo entero como texto corrido, que sigue
// al cursor; «Fragmento aprox», sólo el párrafo o la fórmula del cursor. Se redibujan con cada tecla.
import katex from 'katex';
import 'katex/dist/katex.min.css';
import { useEffect, useMemo, useRef } from 'react';
import { getDoc } from '../editor/docs';
import { useApp } from '../store/app';
import { aHtml, bloques, fragmentoEnCursor, type Formula } from '../vista/aproximada';

const formula: Formula = (tex, enBloque) => {
  try {
    return katex.renderToString(tex, { displayMode: enBloque, throwOnError: false, strict: false, output: 'html' });
  } catch {
    return `<code class="aprox-error">${tex.replace(/</g, '&lt;')}</code>`;
  }
};

// El HTML de cada bloque por su texto: al tipear sólo cambia el bloque del cursor y los demás salen de acá. Se vacía cuando crece
// demasiado (otro archivo, muchas ediciones).
const cache = new Map<string, string>();
function html(texto: string): string {
  let h = cache.get(texto);
  if (h === undefined) {
    if (cache.size > 5000) cache.clear();
    h = aHtml(texto, formula);
    cache.set(texto, h);
  }
  return h;
}

function Pie({ ms }: { ms: number }) {
  return (
    <div className="aprox-pie">
      Aproximado, sin TeX ({ms < 1 ? '<1' : Math.round(ms)} ms): no corta líneas ni páginas como el PDF, no conoce las macros del proyecto ni dibuja
      TikZ. La página real: Vista previa (Ctrl+Alt+F).
    </div>
  );
}

/** «Al instante»: el archivo entero, con el bloque del cursor marcado y a la vista. */
export function VistaCapitulo() {
  const rel = useApp((s) => s.activeRel);
  const epoch = useApp((s) => s.cursorEpoch);
  const hojaRef = useRef<HTMLDivElement>(null);
  const { partes, actual, ms } = useMemo(() => {
    const doc = rel ? getDoc(rel) : undefined;
    if (!doc) return { partes: [] as string[], actual: -1, ms: 0 };
    const t0 = performance.now();
    const bs = bloques(doc.state.doc.toString());
    const pos = doc.state.selection.main.head;
    const actual = bs.findIndex((b) => pos >= b.desde && pos <= b.hasta);
    return { partes: bs.map((b) => html(b.texto)), actual, ms: performance.now() - t0 };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rel, epoch]);

  // El bloque del cursor, a la vista (sin moverse si ya se ve).
  useEffect(() => {
    const el = hojaRef.current?.querySelector<HTMLElement>(`[data-bloque="${actual}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }, [actual]);

  if (!rel) return <div className="vacio">Abrí un archivo del proyecto.</div>;
  return (
    <div className="aprox">
      <div className="aprox-hoja" ref={hojaRef}>
        {partes.map((h, i) => (
          <div key={i} data-bloque={i} className={`aprox-bloque${i === actual ? ' actual' : ''}`} dangerouslySetInnerHTML={{ __html: h }} />
        ))}
        {partes.length === 0 && <span className="aprox-vacio">(archivo vacío)</span>}
      </div>
      <Pie ms={ms} />
    </div>
  );
}

/** «Fragmento aprox»: sólo el párrafo o la fórmula del cursor. */
export function VistaFragmentoAprox() {
  const rel = useApp((s) => s.activeRel);
  const epoch = useApp((s) => s.cursorEpoch);
  const { h, ms } = useMemo(() => {
    const doc = rel ? getDoc(rel) : undefined;
    if (!doc) return { h: '', ms: 0 };
    const t0 = performance.now();
    const frag = fragmentoEnCursor(doc.state.doc.toString(), doc.state.selection.main.head);
    return { h: html(frag.texto), ms: performance.now() - t0 };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rel, epoch]);

  if (!rel) return <div className="vacio">Abrí un archivo y poné el cursor en un párrafo o una fórmula.</div>;
  return (
    <div className="aprox">
      <div className="aprox-hoja" dangerouslySetInnerHTML={{ __html: h || '<span class="aprox-vacio">(párrafo vacío)</span>' }} />
      <Pie ms={ms} />
    </div>
  );
}
