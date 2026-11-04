// Panel del PDF: barra de zoom y navegación y el visor. El visor lo maneja `PdfViewerHost` (imperativo); acá sólo se lo
// crea, se le pasan los bytes cuando el store avisa que hay un PDF nuevo y se refleja su estado en la barra.
//
// Hay dos visores, cada uno en su pestaña: el del documento y el de la vista previa de un fragmento (8.2 del plan). Cada uno
// conserva su página y su zoom mientras se mira el otro (el que no se ve queda con `display: none`; PDF.js lo reacomoda al volver,
// porque el visor sigue el tamaño de su contenedor). Las funciones de SyncTeX (Seguir, el doble clic) son del documento; los botones de salto viven en la barra del medio (`BotonesDeSalto`).
import { useEffect, useRef, useState } from 'react';
import { getBackend } from '../backend';
import { useApp } from '../store/app';
import { pdfActual, PdfViewerHost, type PdfEstado } from './viewer';
import { pdfApi } from './api';
import { VistaCapitulo, VistaFragmentoAprox } from './VistaAproximada';
import { VistaVivo } from './VistaVivo';
import { estaPrendido, useTexpresso } from '../store/texpresso';

const VACIO: PdfEstado = { pagina: 0, paginas: 0, escala: 1, modo: 'ancho' };

export function PdfPane() {
  const areaRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<PdfViewerHost | null>(null);
  const fragAreaRef = useRef<HTMLDivElement>(null);
  const fragHostRef = useRef<PdfViewerHost | null>(null);
  const [estadoDoc, setEstadoDoc] = useState<PdfEstado>(VACIO);
  const [estadoFrag, setEstadoFrag] = useState<PdfEstado>(VACIO);
  const [cargando, setCargando] = useState(false);
  const [paginaTexto, setPaginaTexto] = useState('');
  const pdfRel = useApp((s) => s.pdfRel);
  const pdfEpoch = useApp((s) => s.pdfEpoch);
  const status = useApp((s) => s.build.status);
  const stale = useApp((s) => s.pdfStale);
  const avisar = useApp((s) => s.avisar);
  const seguir = useApp((s) => s.seguir);
  const irAlCodigo = useApp((s) => s.irAlCodigo);
  const setSeguir = useApp((s) => s.setSeguir);
  const vista = useApp((s) => s.vistaPdf);
  const mostrarVista = useApp((s) => s.mostrarVista);
  const fragmento = useApp((s) => s.fragmento);
  // La pestaña «En vivo» existe mientras TeXpresso está prendido: al apagarlo, si se la estaba mirando, se vuelve al documento.
  const tpPrendido = useTexpresso((s) => estaPrendido(s.vista));
  useEffect(() => {
    if (!tpPrendido && useApp.getState().vistaPdf === 'vivo') useApp.getState().mostrarVista('pdf');
  }, [tpPrendido]);

  useEffect(() => {
    const host = new PdfViewerHost(areaRef.current!);
    hostRef.current = host;
    pdfActual.host = host;
    pdfApi.host = host;
    const baja = host.suscribir((e) => setEstadoDoc(e));
    const frag = new PdfViewerHost(fragAreaRef.current!);
    fragHostRef.current = frag;
    const bajaFrag = frag.suscribir((e) => setEstadoFrag(e));
    return () => {
      baja();
      bajaFrag();
      pdfActual.host = null;
      pdfApi.host = null;
      hostRef.current = null;
      fragHostRef.current = null;
      void host.destruir();
      void frag.destruir();
    };
  }, []);

  // Cada vez que el store sube `pdfEpoch` hay un PDF nuevo para mostrar.
  useEffect(() => {
    if (!pdfRel || pdfEpoch === 0) return;
    let vigente = true;
    setCargando(true);
    (async () => {
      try {
        const bytes = await getBackend().readPdf(pdfRel);
        if (vigente) await hostRef.current?.cargar(bytes, `${pdfRel}#${pdfEpoch}`);
        if (vigente) useApp.getState().pdfMostrado();
      } catch (e) {
        avisar(`No se pudo mostrar el PDF: ${e instanceof Error ? e.message : e}`, 'error');
      } finally {
        if (vigente) setCargando(false);
      }
    })();
    return () => {
      vigente = false;
    };
  }, [pdfRel, pdfEpoch, avisar]);

  // Lo mismo con el fragmento: `fragmento.epoch` sube con cada vista previa que compiló.
  useEffect(() => {
    if (!fragmento.pdfRel || fragmento.epoch === 0) return;
    let vigente = true;
    (async () => {
      try {
        const bytes = await getBackend().readPdf(fragmento.pdfRel!);
        if (vigente) await fragHostRef.current?.cargar(bytes, `${fragmento.pdfRel}#${fragmento.epoch}`);
      } catch (e) {
        avisar(`No se pudo mostrar el fragmento: ${e instanceof Error ? e.message : e}`, 'error');
      }
    })();
    return () => {
      vigente = false;
    };
  }, [fragmento.pdfRel, fragmento.epoch, avisar]);

  const enFragmento = vista === 'fragmento';
  const enAproximada = vista === 'aproximada' || vista === 'fragmentoAprox';
  const enVivo = vista === 'vivo';
  const estado = enFragmento ? estadoFrag : estadoDoc;
  useEffect(() => setPaginaTexto(estado.pagina ? String(estado.pagina) : ''), [estado.pagina]);

  // Ctrl + rueda hace zoom (el listener no puede ser pasivo: tiene que impedir el zoom de la página entera).
  useEffect(() => {
    const poner = (el: HTMLElement, host: () => PdfViewerHost | null) => {
      const rueda = (e: WheelEvent) => {
        if (!e.ctrlKey) return;
        e.preventDefault();
        if (e.deltaY < 0) host()?.zoomMas();
        else host()?.zoomMenos();
      };
      el.addEventListener('wheel', rueda, { passive: false });
      return () => el.removeEventListener('wheel', rueda);
    };
    const a = poner(areaRef.current!, () => hostRef.current);
    const b = poner(fragAreaRef.current!, () => fragHostRef.current);
    return () => {
      a();
      b();
    };
  }, []);

  const h = enFragmento ? fragHostRef.current : hostRef.current;
  const teclas = (e: React.KeyboardEvent) => {
    if (!h) return;
    if (e.ctrlKey && (e.key === '=' || e.key === '+')) h.zoomMas();
    else if (e.ctrlKey && e.key === '-') h.zoomMenos();
    else if (e.ctrlKey && e.key === '0') h.setEscala(1);
    else if (!e.ctrlKey && !e.altKey && e.key.toLowerCase() === 'w') h.ajustarAncho();
    else return;
    e.preventDefault();
  };

  const hayPdf = estado.paginas > 0;
  const hayFragmento = fragmento.estado !== 'idle' || fragmento.epoch > 0;
  return (
    <div className="pdf-panel">
      <div className="pdf-pestanas" role="tablist">
        <button role="tab" aria-selected={vista === 'pdf'} className={`pdf-pestana${vista === 'pdf' ? ' activa' : ''}`} onClick={() => mostrarVista('pdf')}>
          Documento
        </button>
        <button
          role="tab"
          aria-selected={vista === 'aproximada'}
          className={`pdf-pestana${vista === 'aproximada' ? ' activa' : ''}`}
          onClick={() => mostrarVista('aproximada')}
          title="El archivo entero, dibujado al instante y sin TeX (aproximado, sin páginas): sigue al cursor"
        >
          Al instante
        </button>
        <button
          role="tab"
          aria-selected={vista === 'fragmentoAprox'}
          className={`pdf-pestana${vista === 'fragmentoAprox' ? ' activa' : ''}`}
          onClick={() => mostrarVista('fragmentoAprox')}
          title="Sólo el párrafo o la fórmula del cursor, dibujado al instante y sin TeX (aproximado)"
        >
          Fragmento aprox
        </button>
        {tpPrendido && (
          <button
            role="tab"
            aria-selected={enVivo}
            className={`pdf-pestana pdf-pestana-vivo${enVivo ? ' activa' : ''}`}
            onClick={() => mostrarVista('vivo')}
            title="La página que TeXpresso ya tiene dibujada, mientras escribís (aproximada: lo que se entrega sale de latexmk). Clic en la página: al código."
          >
            ⚡ En vivo
          </button>
        )}
        {hayFragmento && (
          <button role="tab" aria-selected={enFragmento} className={`pdf-pestana${enFragmento ? ' activa' : ''}`} onClick={() => mostrarVista('fragmento')} title="Vista previa del fragmento (Ctrl+Alt+F)">
            Fragmento{fragmento.estado === 'compilando' ? ' …' : fragmento.estado === 'fail' ? ' ✘' : ''}
          </button>
        )}
      </div>
      {vista === 'aproximada' && <VistaCapitulo />}
      {vista === 'fragmentoAprox' && <VistaFragmentoAprox />}
      {enVivo && <VistaVivo />}
      <div className="pdf-barra" style={enAproximada || enVivo ? { display: 'none' } : undefined}>
        <button className="icono-boton" disabled={!hayPdf} onClick={() => h?.anterior()} title="Página anterior">◀</button>
        <input
          className="pagina"
          value={paginaTexto}
          disabled={!hayPdf}
          onChange={(e) => setPaginaTexto(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') h?.irA(Number(paginaTexto) || 1);
            e.stopPropagation();
          }}
          title="Ir a la página"
        />
        <span className="de">/ {estado.paginas || '–'}</span>
        <button className="icono-boton" disabled={!hayPdf} onClick={() => h?.siguiente()} title="Página siguiente">▶</button>
        <span className="sep" />
        <button className="icono-boton" disabled={!hayPdf} onClick={() => h?.zoomMenos()} title="Alejar (Ctrl+-)">−</button>
        <span className="zoom">{Math.round(estado.escala * 100)} %</span>
        <button className="icono-boton" disabled={!hayPdf} onClick={() => h?.zoomMas()} title="Acercar (Ctrl+=)">+</button>
        <span className="sep" />
        <button className={`boton chico${seguir ? ' activo' : ''}`} disabled={enFragmento} onClick={() => setSeguir(!seguir)} title="Seguir al cursor: el PDF acompaña al cursor del editor">
          Seguir
        </button>
        <span className="sep" />
        <button className={`boton chico${estado.modo === 'ancho' ? ' activo' : ''}`} disabled={!hayPdf} onClick={() => h?.ajustarAncho()} title="Ajustar al ancho de la página (W)">
          Ancho
        </button>
        {(cargando || (enFragmento && fragmento.estado === 'compilando')) && <span className="cargando">{enFragmento ? 'Compilando…' : 'Cargando…'}</span>}
      </div>
      <div
        className={`pdf-area${stale ? ' desactualizado' : ''}`}
        ref={areaRef}
        style={vista !== 'pdf' ? { display: 'none' } : undefined}
        tabIndex={0}
        onKeyDown={teclas}
        // Doble clic (o Ctrl+clic) en el PDF: al código (6.3 del plan).
        onDoubleClick={(e) => {
          const p = hostRef.current?.puntoDeEvento(e.nativeEvent);
          if (p) void irAlCodigo(p);
        }}
        onClick={(e) => {
          if (!e.ctrlKey) return;
          const p = hostRef.current?.puntoDeEvento(e.nativeEvent);
          if (p) void irAlCodigo(p);
        }}
      >
        {/* Los contenedores de cada visor los arma `PdfViewerHost` adentro de este elemento. */}
        {!estadoDoc.paginas && !cargando && (
          <div className="vacio">{status === 'running' ? 'Compilando…' : 'Todavía no hay PDF. Compilá con Ctrl+Enter.'}</div>
        )}
      </div>
      <div className="pdf-area" ref={fragAreaRef} style={enFragmento ? undefined : { display: 'none' }} tabIndex={0} onKeyDown={teclas}>
        {!estadoFrag.paginas && (
          <div className="vacio">
            {fragmento.estado === 'compilando'
              ? 'Compilando el fragmento…'
              : fragmento.estado === 'fail'
                ? 'El fragmento no compiló: mirá el log o los problemas.'
                : 'Elegí un entorno o una selección y pedí la vista previa (Ctrl+Alt+F).'}
          </div>
        )}
      </div>
      {stale && estadoDoc.paginas > 0 && !enFragmento && <div className="pdf-aviso">El último build no terminó bien: este es el último PDF bueno.</div>}
      {enFragmento && fragmento.estado === 'fail' && <div className="pdf-aviso">El último fragmento no compiló: este es el último que salió bien.</div>}
    </div>
  );
}
