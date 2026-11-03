// Panel Esquema (10.1 del plan): capítulos y secciones de todo el proyecto en el orden del documento, con su etiqueta. Un clic lleva a
// la sección. Antes cada capítulo tenía la casilla del panel Partes (elegir capítulos a mano); salió con ese panel: qué capítulos
// compila cada perfil se elige en la pantalla de perfiles.
import { useApp } from '../store/app';

const NIVELES = ['parte', 'capítulo', 'sección', 'subsección', 'subsubsección', 'párrafo', 'subpárrafo'];

export function EsquemaPanel() {
  const project = useApp((s) => s.project);
  const simbolos = useApp((s) => s.simbolos);
  const activeRel = useApp((s) => s.activeRel);
  const ir = useApp((s) => s.irAUbicacion);

  if (!project) return <div className="vacio">Abrí una carpeta para empezar.</div>;
  if (!simbolos) return <div className="vacio">Leyendo el proyecto…</div>;
  if (simbolos.sections.length === 0) return <div className="vacio">No hay capítulos ni secciones en los archivos del proyecto.</div>;

  const etiquetas = simbolos.labels.length;

  return (
    <div className="esquema">
      <div className="esquema-lista">
        {simbolos.sections.map((s, i) => (
          <div
            key={`${s.file}:${s.line}:${i}`}
            className={`esquema-item nivel-${Math.min(s.level, 4)}${s.file === activeRel ? ' del-archivo' : ''}`}
            style={{ paddingLeft: 8 + Math.max(0, s.level - 1) * 14 }}
            title={`${NIVELES[s.level] ?? 'sección'} · ${s.file}:${s.line}`}
            onClick={() => void ir(s.file, s.line)}
          >
            <span className="titulo">{s.title || '(sin título)'}</span>
            {s.label && <span className="etiqueta">{s.label}</span>}
          </div>
        ))}
      </div>
      <div className="partes-pie">
        {simbolos.sections.length} secciones · {etiquetas} etiquetas · {simbolos.scan_ms} ms
      </div>
    </div>
  );
}
