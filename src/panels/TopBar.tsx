// Barra superior: el proyecto, el perfil activo, Compilar / Cancelar y el resultado del último build. Todo lo demás es del perfil y
// se cambia en la pantalla de perfiles (⚙).
import { useEffect, useState } from 'react';
import type { PartsSel, Tipico } from '../backend';
import { perfilActivo, useApp } from '../store/app';
import { lineaDeBuild, textoDeTipico } from '../store/barra';
import { abrirPantallaProyecto } from '../store/proyecto';
import { textoDeProgreso } from '../store/figuras';
import { resumen } from '../store/problemas';
import { siguienteTema, textoDelTema, useTema } from '../store/tema';

/** Los segundos que lleva el build (desde que se pidió) y, al lado, lo típico de ese perfil según el historial. */
function Cronometro({ desde, tipico }: { desde: number | null; tipico: Tipico | undefined }) {
  const [seg, setSeg] = useState(0);
  useEffect(() => {
    const t0 = desde ?? Date.now();
    const actualizar = () => setSeg((Date.now() - t0) / 1000);
    actualizar();
    const t = setInterval(actualizar, 250);
    return () => clearInterval(t);
  }, [desde]);
  const t = textoDeTipico(tipico, seg);
  return (
    <>
      {seg.toFixed(0)} s{t && <span className="tipico"> ({t})</span>}
    </>
  );
}

const nombreCorto = (n: string) => n.split('/').pop() ?? n;

/** "Desnormalización", "3 partes" o "todas las partes": lo que compila el perfil activo, en una frase corta. */
export function resumenDePartes(parts: PartsSel | undefined, seleccion: string[], total: number, actual: string | null = null, lista: string[] = []): string {
  if (parts === 'current' || parts === 'list') {
    const todas = [...(parts === 'current' && actual ? [actual] : []), ...lista.filter((p) => p !== actual)];
    if (todas.length === 0) return parts === 'current' ? 'el capítulo donde estés' : 'ninguna parte';
    return todas.length === 1 ? nombreCorto(todas[0]) : `${nombreCorto(todas[0])} y ${todas.length - 1} más`;
  }
  if (parts === 'selection') {
    if (seleccion.length === 0) return 'ninguna parte';
    if (seleccion.length === 1) return nombreCorto(seleccion[0]);
    return `${seleccion.length} de ${total} partes`;
  }
  if (parts === 'all') return total > 0 ? `todas las partes (${total})` : 'todo el documento';
  return '';
}

export function TopBar() {
  const project = useApp((s) => s.project);
  const rootFile = useApp((s) => s.rootFile);
  const build = useApp((s) => s.build);
  const abrirCarpeta = useApp((s) => s.abrirCarpeta);
  const elegirPerfil = useApp((s) => s.elegirPerfil);
  const compilar = useApp((s) => s.compilar);
  const cancelar = useApp((s) => s.cancelarBuild);
  const previsualizar = useApp((s) => s.previsualizarFragmento);
  const mostrarPanel = useApp((s) => s.mostrarPanel);
  const figProgreso = useApp((s) => s.figProgreso);
  const exportando = useApp((s) => s.exportando);
  const exportPendiente = useApp((s) => s.exportPendiente);
  const mostrarPanelInferior = useApp((s) => s.mostrarPanelInferior);
  const avisar = useApp((s) => s.avisar);
  const tema = useTema((s) => s.tema);
  const alternarTema = useTema((s) => s.alternar);
  const corriendo = build.status === 'running';
  // Exportar es un trabajo más de la cola: mientras corre o espera, Cancelar lo corta.
  const ocupado = corriendo || exportando !== null;
  const config = project?.config ?? null;
  const estado = project?.state;
  const activo = perfilActivo(project);
  const visibles = config?.profiles.filter((p) => !p.hidden) ?? [];
  // Siempre la etiqueta, también durante el build (antes mostraba el id con «…»).
  const etiquetaPerfil = (id: string) => config?.profiles.find((p) => p.id === id)?.label ?? id;
  const tipicos = useApp((s) => s.tipicos);
  // Lo que compila el build en curso (puede no ser el perfil activo: quedó en cola o lo pidió un menú).
  const enCurso = config?.profiles.find((p) => p.id === build.perfil);
  const partesEnCurso = enCurso ? resumenDePartes(enCurso.parts, estado?.selection ?? [], project?.parts.length ?? 0, estado?.currentPart ?? null, enCurso.partList) : '';

  return (
    <div className="barra-superior">
      <button className="boton" onClick={() => abrirCarpeta().catch((e) => avisar(String(e), 'error'))}>
        Abrir carpeta…
      </button>
      {project && (
        <>
          <span className="proyecto" title={project.root}>{project.name}</span>
          <button
            className="icono-boton config-proyecto"
            onClick={() => void abrirPantallaProyecto()}
            title="Configurar el proyecto: documento principal, partes, perfiles, salidas, variables"
            aria-label="Configurar el proyecto"
          >
            ⚙
          </button>
          {/* La barra sólo elige el perfil y compila: todo lo demás (capítulos, piezas, borrador, si compila al guardar, adónde va el
              PDF) es del perfil y se cambia en la pantalla de perfiles (⚙). Antes repetía esas opciones y se contradecían. */}
          {config && estado && (
            <>
              <label className="raiz">
                Perfil
                <select
                  className="perfil-select"
                  value={estado.activeProfile ?? ''}
                  onChange={(e) => void elegirPerfil(e.target.value)}
                  title="Lo que compila Ctrl+S y Compilar"
                  style={activo?.color ? { color: activo.color, borderColor: activo.color } : undefined}
                >
                  {visibles.map((p) => (
                    <option key={p.id} value={p.id} style={p.color ? { color: p.color } : undefined}>
                      {p.color ? '● ' : ''}
                      {etiquetaPerfil(p.id)}
                    </option>
                  ))}
                </select>
              </label>
              {ocupado ? (
                <button className="boton peligro" onClick={() => void cancelar()}>■ Cancelar</button>
              ) : (
                <button className="boton primario" onClick={() => void compilar()} disabled={!rootFile} title="Guardar todo y compilar el perfil activo (Ctrl+Enter)">
                  ▶ Compilar
                </button>
              )}
              {/* Una acción del programa, no del perfil: compila sólo lo que está bajo el cursor (la figura, la ecuación, el párrafo) o lo
                  seleccionado, y lo muestra en la pestaña Fragmento del visor. */}
              <button
                className="boton"
                onClick={() => void previsualizar()}
                disabled={!rootFile}
                title="Compilar sólo lo que está bajo el cursor (o lo seleccionado) y verlo en la pestaña Fragmento (Ctrl+Alt+F)"
              >
                Vista previa
              </button>
            </>
          )}
          {(build.status === 'ok' || build.status === 'fail') && resumen(build.errors ?? 0, build.warnings ?? 0, 0) && (
            <button className={`problemas-resumen ${build.errors ? 'error' : 'warning'}`} onClick={() => mostrarPanelInferior('problemas')} title="Ver los problemas">
              {resumen(build.errors ?? 0, build.warnings ?? 0, 0)}
            </button>
          )}
          {figProgreso && (
            <button className="figuras-barra" onClick={() => mostrarPanel('figuras')} title="Ver el panel Figuras">
              {textoDeProgreso(figProgreso)}
            </button>
          )}
          {(exportando || exportPendiente) && (
            <button className="exportar-barra" onClick={() => mostrarPanelInferior('log')} title="Ver el log de la exportación">
              {exportando
                ? `Exportando ${Math.min(exportando.hechas + 1, exportando.total)}/${exportando.total}${exportando.parte ? ` · listo «${nombreCorto(exportando.parte)}»` : ''}`
                : 'Exportación en espera'}
            </button>
          )}
          <span className={`estado-build ${build.status}`}>
            {build.status === 'running' && (
              <>
                {figProgreso && build.id === null
                  ? 'Esperando las figuras…'
                  : lineaDeBuild({ perfil: build.perfil ? etiquetaPerfil(build.perfil) : '…', partes: partesEnCurso, paso: build.paso, figura: build.figura })}{' '}
                <Cronometro desde={build.inicioMs} tipico={build.perfil ? tipicos[build.perfil] : undefined} />
                {build.pendiente && <span className="pendiente" title="Se compila una vez más cuando termine"> · 1 pendiente</span>}
              </>
            )}
            {build.status === 'ok' && (
              <>
                ✔ {((build.ms ?? 0) / 1000).toFixed(1)} s{build.passes ? ` · ${build.passes} pasada${build.passes === 1 ? '' : 's'}` : ''}
                {build.totalMs ? (
                  <span title="Desde que se pidió la compilación hasta que el PDF nuevo quedó en pantalla (incluye guardar, esperar y cargar el PDF)">
                    {' '}
                    · {(build.totalMs / 1000).toFixed(1)} s en total
                    {build.arrancoMs && build.terminoMs && build.inicioMs
                      ? ` (espera ${((build.arrancoMs - build.inicioMs) / 1000).toFixed(1)} · TeX ${((build.terminoMs - build.arrancoMs) / 1000).toFixed(1)} · PDF ${((build.inicioMs + build.totalMs - build.terminoMs) / 1000).toFixed(1)})`
                      : ''}
                  </span>
                ) : null}
                {build.repaired && ' · reparado'}
                {build.borrador && <span className="borrador" title="El PDF tiene cajas del tamaño de cada imagen en lugar de las imágenes"> · imágenes en borrador</span>}
              </>
            )}
            {build.status === 'fail' && <>✘ falló (código {build.code ?? '?'})</>}
            {build.status === 'cancelled' && <>Cancelado</>}
          </span>
        </>
      )}
      <button
        className={`icono-boton tema-boton${project ? '' : ' solo'}`}
        onClick={() => void alternarTema()}
        title={`Tema: ${textoDelTema(tema)}. Clic para pasar a ${textoDelTema(siguienteTema(tema))}.`}
        aria-label={`Tema: ${textoDelTema(tema)}`}
      >
        {tema === 'claro' ? '☀' : tema === 'oscuro' ? '🌙' : '🖥'}
      </button>
    </div>
  );
}
