// Pestaña «En vivo» del panel PDF: la página que TeXpresso (en WSL) ya tiene dibujada, mandada como imagen mientras se escribe.
//
// No es el PDF: lo que se entrega sale de latexmk y se ve en «Documento». Acá no hay texto seleccionable ni búsqueda (el PDF de
// latexmk cubre eso); sí hay clic → código (TeXpresso resuelve el SyncTeX de su propia compilación) y ◀ ▶ para cambiar de página.
// La imagen sólo se pide mientras la pestaña está a la vista: dibujarla y codificarla le cuesta a TeXpresso tiempo que no hace falta.
import { useEffect, useRef, useState } from 'react';
import { getBackend } from '../backend';
import { estaPrendido, useTexpresso } from '../store/texpresso';
import { anchoPedido, puntoDeClic, textoPagina } from './vivo';

export function VistaVivo() {
  const areaRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const vista = useTexpresso((s) => s.vista);
  const disp = useTexpresso((s) => s.disp);
  const imagen = useTexpresso((s) => s.imagen);
  const prendido = estaPrendido(vista);
  const [ancho, setAncho] = useState(0);

  // El ancho del panel (con debounce: arrastrar la barra separadora no tiene que pedir una imagen por píxel).
  useEffect(() => {
    const el = areaRef.current;
    if (!el) return;
    let t: ReturnType<typeof setTimeout> | null = null;
    const medir = () => setAncho(anchoPedido(el.clientWidth, window.devicePixelRatio));
    medir();
    const ro = new ResizeObserver(() => {
      if (t) clearTimeout(t);
      t = setTimeout(medir, 150);
    });
    ro.observe(el);
    return () => {
      if (t) clearTimeout(t);
      ro.disconnect();
    };
  }, []);

  // Mientras la pestaña se ve y TeXpresso está prendido, se le pide la imagen; al irse (o apagarlo) se la deja de pedir.
  useEffect(() => {
    if (!prendido || ancho === 0) return;
    void getBackend().texpressoVivo(ancho).catch(() => {});
    return () => {
      void getBackend().texpressoVivo(0).catch(() => {});
    };
  }, [prendido, ancho]);

  const alClic = (e: React.MouseEvent<HTMLImageElement>) => {
    const img = imgRef.current;
    if (!img || !imagen) return;
    const r = img.getBoundingClientRect();
    const p = puntoDeClic(e.clientX - r.left, e.clientY - r.top, r.width, r.height, imagen.ancho, imagen.alto);
    if (p) void getBackend().texpressoVivoClic(p.x, p.y).catch(() => {});
  };

  const sinParche = prendido && disp?.completo === true && !disp.vivo;
  const vacio = !prendido
    ? 'Prendé ⚡ TeXpresso en la barra para ver el documento acá mientras escribís.'
    : sinParche
      ? 'Este TeXpresso no tiene el parche de la vista «En vivo»: se sigue viendo en su ventana. Para aplicarlo, ver INSTALAR.md («Vista previa en vivo»).'
      : vista?.fase === 'iniciando'
        ? 'Iniciando TeXpresso…'
        : vista?.fase === 'caido'
          ? 'TeXpresso se cayó: ver el botón de la barra.'
          : 'Esperando la primera página…';
  const estado = vista?.fase === 'listo' ? `listo${vista.errores ? ` · ${vista.errores} error${vista.errores === 1 ? '' : 'es'}` : vista.ultimoMs !== null ? ` · ${vista.ultimoMs} ms` : ''}` : (vista?.fase ?? '');

  return (
    <div className="vivo">
      <div className="pdf-barra vivo-barra">
        <button className="icono-boton" disabled={!imagen || imagen.pagina <= 0} onClick={() => void getBackend().texpressoPaginaVivo(-1).catch(() => {})} title="Página anterior">
          ◀
        </button>
        <span className="vivo-pagina">{textoPagina(imagen?.pagina ?? null) || '–'}</span>
        {/* ▶ no se deshabilita por el total de páginas: TeXpresso compila hasta la página que se le pide y no sabe cuántas hay. */}
        <button className="icono-boton" disabled={!imagen} onClick={() => void getBackend().texpressoPaginaVivo(1).catch(() => {})} title="Página siguiente">
          ▶
        </button>
        <span className="sep" />
        <span className="vivo-estado" title="Vista previa de TeXpresso: aproximada. Lo que se entrega sale de latexmk (pestaña Documento).">
          ⚡ {estado}
        </span>
      </div>
      <div className="vivo-area" ref={areaRef}>
        {imagen && prendido ? (
          <img ref={imgRef} className="vivo-imagen" src={imagen.src} width={imagen.ancho} height={imagen.alto} alt={`Página ${imagen.pagina + 1} de TeXpresso`} onClick={alClic} title="Clic: ir al código" draggable={false} />
        ) : (
          <div className="vacio vivo-vacio">{vacio}</div>
        )}
      </div>
    </div>
  );
}
