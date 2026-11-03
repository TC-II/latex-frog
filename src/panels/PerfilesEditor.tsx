// La pantalla de perfiles (la maqueta acordada con el autor del libro): arriba los perfiles como botones de color; a la izquierda el
// nombre, el color y las piezas del documento en el orden del libro; a la derecha cómo se compila y adónde va el PDF; abajo lo que
// sale. Edita la copia de `latexviewer.json` de la pantalla Proyecto, que es la que escribe «Guardar».
import { useEffect, useState } from 'react';
import { getBackend } from '../backend';
import { useApp } from '../store/app';
import { useProyecto } from '../store/proyecto';
import { fijarPerfil, ID_DE_PERFIL, quitarPerfil, type ConfigJson, type PerfilJson } from '../store/proyectoConfig';
import { aPerfil, carpetaRelativa, COLORES, filas, nombreCorto, seleccionDe } from '../store/perfiles';

const SALIDA_POR_OMISION = { dir: '../PDF', pattern: '{nombre}.pdf' };

/** Un id de perfil libre a partir de un nombre: ASCII, sin espacios. */
function idPara(nombre: string, usados: string[]): string {
  const base = nombre.normalize('NFD').replace(/[^\w-]+/g, '-').replace(/^-+|-+$/g, '').toLowerCase() || 'perfil';
  let id = base;
  for (let i = 2; usados.includes(id); i++) id = `${base}-${i}`;
  return ID_DE_PERFIL.test(id) ? id : `perfil-${usados.length + 1}`;
}

export function PerfilesEditor({ config }: { config: ConfigJson }) {
  const editar = useProyecto((s) => s.editar);
  const propuesta = useProyecto((s) => s.propuesta);
  const project = useApp((s) => s.project);
  // Las piezas vienen con la detección, que es lo que abre esta pantalla: así nunca se arma un perfil sin verlas.
  const piezas = propuesta?.piezas ?? project?.piezas ?? [];
  const partes = propuesta?.partes ?? project?.parts.map((p) => p.name) ?? [];
  const ids = Object.keys(config.profiles ?? {});
  const [elegido, setElegido] = useState<string>(ids[0] ?? '');
  // La última salida de cada perfil, para mostrarla en gris cuando se tilda «sólo para ver en la app» y devolverla al destildar.
  const [salidas, setSalidas] = useState<Record<string, NonNullable<PerfilJson['output']>>>({});
  const [errorCarpeta, setErrorCarpeta] = useState<string | null>(null);
  const id = ids.includes(elegido) ? elegido : (ids[0] ?? '');
  const p: PerfilJson = config.profiles?.[id] ?? {};
  const color = p.color ?? 'var(--borde)';
  const cambiar = (parche: PerfilJson) => editar((c) => fijarPerfil(c, id, parche));
  const sel = seleccionDe(p);
  const omit = p.omit ?? [];
  const soloApp = !p.output;
  const salida = p.output ?? salidas[id] ?? SALIDA_POR_OMISION;
  const variables = Object.entries(config.variables ?? {});

  // Un perfil nuevo arranca vacío: ninguna pieza ni capítulo tildado. Lo arma la persona.
  const agregar = () => {
    const nuevo = idPara(`perfil ${ids.length + 1}`, ids);
    // Por omisión: figuras del cache (lo rápido) y sin pie, si el proyecto tiene una variable con la opción «ninguno».
    const sinPie = Object.fromEntries(
      Object.entries(config.variables ?? {})
        .filter(([, v]) => (v.choices ?? []).includes('ninguno'))
        .map(([n]) => [n, 'ninguno']),
    );
    const vacio: PerfilJson = {
      label: `Perfil ${ids.length + 1}`,
      color: COLORES[ids.length % COLORES.length],
      parts: 'list',
      partList: [],
      omit: piezas.map((x) => x.clave),
      externalize: true,
      vars: sinPie,
    };
    editar((c) => fijarPerfil(c, nuevo, vacio));
    setElegido(nuevo);
  };
  // Sin perfiles, la pantalla abre con uno vacío ya creado (y no con un aviso de que no hay).
  useEffect(() => {
    if (ids.length === 0 && propuesta) agregar();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids.length, propuesta]);
  const elegirCarpeta = async () => {
    const abs = await getBackend().pickFolder();
    if (!abs || !project) return;
    const rel = carpetaRelativa(project.root, abs);
    if (rel === null) {
      setErrorCarpeta('Esa carpeta está en otra unidad que el proyecto: elegí una en la misma.');
      return;
    }
    setErrorCarpeta(null);
    cambiar({ output: { ...salida, dir: rel } });
  };


  return (
    <div className="perfiles">
      <label className="perfiles-motor" title="pdflatex es el clásico y el más rápido. xelatex y lualatex permiten usar las fuentes instaladas en Windows (OTF/TTF) y Unicode directo, pero son más lentos. Vale para todo el proyecto.">
        Motor
        <select value={config.engine ?? 'pdflatex'} onChange={(e) => editar((c) => ({ ...c, engine: e.target.value }))}>
          <option value="pdflatex">pdflatex (el clásico, más rápido)</option>
          <option value="xelatex">xelatex (fuentes del sistema)</option>
          <option value="lualatex">lualatex (fuentes del sistema)</option>
        </select>
      </label>
      <div className="perfiles-chips">
        {ids.map((k) => {
          const q = config.profiles?.[k] ?? {};
          return (
            <button key={k} className={`perfil-chip${k === id ? ' elegido' : ''}`} style={k === id ? { borderColor: q.color ?? 'var(--acento)' } : undefined} onClick={() => setElegido(k)}>
              <span className="perfil-punto" style={{ background: q.color ?? 'var(--texto-suave)' }} />
              {q.label ?? k}
            </button>
          );
        })}
        <button className="perfil-chip" onClick={agregar} title="Un perfil nuevo, copia del elegido">
          + Perfil
        </button>
      </div>

      {id && (
        <>
          <div className="perfiles-grilla">
            <div className="perfiles-tarjeta">
              <div className="perfiles-fila">
                <input className="perfiles-nombre" value={p.label ?? id} onChange={(e) => cambiar({ label: e.target.value })} aria-label="Nombre del perfil" />
                {COLORES.map((c) => (
                  <button
                    key={c}
                    className={`perfil-punto grande${p.color === c ? ' elegido' : ''}`}
                    style={{ background: c }}
                    onClick={() => cambiar({ color: c })}
                    aria-label={`Color ${c}`}
                  />
                ))}
                <button className="boton chico peligro" onClick={() => editar((c) => quitarPerfil(c, id))} disabled={ids.length <= 1} title="Borrar este perfil">
                  ✕
                </button>
              </div>
              <div className="perfiles-etiqueta">Piezas, en el orden del libro</div>
              {filas(piezas, partes).map((f) => {
                if (f.tipo === 'pieza') {
                  return (
                    <label key={f.clave} className="perfiles-pieza">
                      <input type="checkbox" style={{ accentColor: color }} checked={!omit.includes(f.clave)} onChange={(e) => cambiar({ omit: e.target.checked ? omit.filter((x) => x !== f.clave) : [...omit, f.clave] })} />
                      {f.nombre}
                    </label>
                  );
                }
                if (f.tipo === 'actual') {
                  return (
                    <label key="actual" className="perfiles-pieza">
                      <input type="checkbox" style={{ accentColor: color }} checked={sel.actual && !sel.todos} disabled={sel.todos} onChange={(e) => cambiar(aPerfil({ ...sel, actual: e.target.checked }))} />
                      <strong>El capítulo donde estoy</strong>
                    </label>
                  );
                }
                if (f.tipo === 'todos') {
                  return (
                    <label key="todos" className="perfiles-pieza">
                      <input type="checkbox" style={{ accentColor: color }} checked={sel.todos} onChange={(e) => cambiar(aPerfil({ ...sel, todos: e.target.checked }))} />
                      <strong>Todos los capítulos</strong>
                    </label>
                  );
                }
                return (
                  <label key={f.parte} className={`perfiles-pieza capitulo${sel.todos ? ' gris' : ''}`}>
                    <input
                      type="checkbox"
                      style={{ accentColor: color }}
                      checked={sel.todos || sel.lista.includes(f.parte)}
                      disabled={sel.todos}
                      onChange={(e) => cambiar(aPerfil({ ...sel, lista: e.target.checked ? [...sel.lista, f.parte] : sel.lista.filter((x) => x !== f.parte) }))}
                    />
                    {nombreCorto(f.parte)}
                  </label>
                );
              })}
            </div>

            <div>
              <div className="perfiles-tarjeta">
                <div className="perfiles-etiqueta">Cómo</div>
                <label className="perfiles-campo">
                  Figuras
                  <select value={p.externalize ? 'cache' : 'cero'} onChange={(e) => cambiar(e.target.value === 'cache' ? { externalize: true, cleanBefore: false } : { externalize: false, cacheOnly: false, cleanBefore: true })}>
                    <option value="cache">Del cache (rápido)</option>
                    <option value="cero">Desde cero</option>
                  </select>
                </label>
                {variables.map(([nombre, v]) => (
                  <label key={nombre} className="perfiles-campo">
                    {v.label ?? nombre}
                    <select value={p.vars?.[nombre] ?? ''} onChange={(e) => cambiar({ vars: { ...(p.vars ?? {}), [nombre]: e.target.value } })}>
                      <option value="">(el de la máquina{v.default ? `: ${v.default}` : ''})</option>
                      {(v.choices ?? []).map((o) => (
                        <option key={o} value={o}>{o}</option>
                      ))}
                    </select>
                  </label>
                ))}
                <label className="perfiles-pieza">
                  <input type="checkbox" checked={p.draftImages === true} onChange={(e) => cambiar({ draftImages: e.target.checked })} />
                  Imágenes en borrador
                </label>
                <label className="perfiles-campo">
                  Compila
                  <select
                    value={p.autoCompile === false ? 'manual' : p.compileOn === 'idle' ? 'idle' : 'save'}
                    onChange={(e) => {
                      const v = e.target.value;
                      cambiar(v === 'manual' ? { autoCompile: false, compileOn: undefined } : { autoCompile: true, compileOn: v === 'idle' ? 'idle' : undefined });
                    }}
                  >
                    <option value="save">al guardar</option>
                    <option value="idle">al dejar de tipear</option>
                    <option value="manual">sólo con el botón Compilar</option>
                  </select>
                </label>
              </div>

              <div className="perfiles-tarjeta">
                <div className="perfiles-etiqueta">Salida</div>
                <div className="perfiles-ayuda">El PDF siempre se ve en la app.</div>
                <label className="perfiles-pieza">
                  <input
                    type="checkbox"
                    checked={!soloApp}
                    onChange={(e) => {
                      if (!e.target.checked) {
                        if (p.output) setSalidas((s) => ({ ...s, [id]: p.output! }));
                        cambiar({ output: undefined });
                      } else {
                        cambiar({ output: salida });
                      }
                    }}
                  />
                  Guardar también una copia en una carpeta
                </label>
                <div className={soloApp ? 'perfiles-gris' : ''}>
                  <label className="perfiles-campo">
                    Archivos
                    <select
                      value={salida.perPart ? 'por' : 'uno'}
                      onChange={(e) => {
                        const porParte = e.target.value === 'por';
                        const pattern = porParte && !/\{(n|cap|nombre)/.test(salida.pattern) ? '{cap:02} - {nombre}.pdf' : salida.pattern;
                        cambiar({ output: { ...salida, perPart: porParte || undefined, pattern }, ...(porParte ? { autoCompile: false } : {}) });
                      }}
                    >
                      <option value="uno">Un solo PDF</option>
                      <option value="por">Un PDF por capítulo</option>
                    </select>
                  </label>
                  <label className="perfiles-campo">
                    Carpeta
                    <span className="perfiles-fila">
                      <input value={salida.dir} onChange={(e) => cambiar({ output: { ...salida, dir: e.target.value } })} placeholder="Elegí una carpeta" />
                      <button className="boton chico" onClick={() => void elegirCarpeta()}>
                        Elegir…
                      </button>
                    </span>
                  </label>
                  {errorCarpeta && <div className="proy-error">{errorCarpeta}</div>}
                  <label className="perfiles-campo">
                    Nombre
                    <input value={salida.pattern} onChange={(e) => cambiar({ output: { ...salida, pattern: e.target.value } })} />
                  </label>
                  <div className="perfiles-ayuda">{'{n}'} consecutivo · {'{cap}'} número del capítulo · {'{nombre}'} de la pieza · {'{perfil}'}</div>
                </div>
              </div>
            </div>
          </div>

        </>
      )}
    </div>
  );
}
