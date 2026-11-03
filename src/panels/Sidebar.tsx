// Panel izquierdo con pestañas: Archivos, Esquema, Buscar y Figuras. La pestaña Partes (elegir capítulos a mano) salió: qué capítulos
// compila cada perfil se elige en la pantalla de perfiles, y tenerlo en dos lugares se contradecía.
import { useApp, type PanelIzquierdo } from '../store/app';
import { EsquemaPanel } from './EsquemaPanel';
import { FigurasPanel } from './FigurasPanel';
import { FileTree } from './FileTree';
import { SearchPanel } from './SearchPanel';

const PESTANAS: { id: PanelIzquierdo; titulo: string }[] = [
  { id: 'archivos', titulo: 'Archivos' },
  { id: 'esquema', titulo: 'Esquema' },
  { id: 'buscar', titulo: 'Buscar' },
  { id: 'figuras', titulo: 'Figuras' },
];

export function Sidebar() {
  const panel = useApp((s) => s.panelIzq);
  const mostrar = useApp((s) => s.mostrarPanel);
  return (
    <div className="lv-sidebar">
      <div className="sidebar-pestanas" role="tablist">
        {PESTANAS.map((p) => (
          <button key={p.id} role="tab" aria-selected={panel === p.id} className={panel === p.id ? 'activa' : ''} onClick={() => mostrar(p.id)}>
            {p.titulo}
          </button>
        ))}
      </div>
      <div className="sidebar-cuerpo">
        {panel === 'archivos' || panel === 'partes' ? (
          <FileTree />
        ) : panel === 'esquema' ? (
          <EsquemaPanel />
        ) : panel === 'buscar' ? (
          <SearchPanel />
        ) : (
          <FigurasPanel />
        )}
      </div>
    </div>
  );
}
