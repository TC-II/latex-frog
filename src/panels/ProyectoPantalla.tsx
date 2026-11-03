// Pantalla «Proyecto» (F8b): la configuración del proyecto sin tocar el JSON. Se abre sola si no hay `latexviewer.json` y después desde el
// menú ⋯ o la paleta. Cada sección dice qué detectó la app y por qué, y se puede cambiar; «Guardar» muestra el diff antes de escribir.
import { useEffect, useMemo, useState } from 'react';
import type { FlagDetectado, UsoDeFlag } from '../backend';
import { diffCompacto, resumenDeDiff } from '../store/diff';
import { duracion } from '../store/historial';
import { useProyecto } from '../store/proyecto';
import { PerfilesEditor } from './PerfilesEditor';
import {
  aTexto,
  comandosDeParte,
  fijarComandosDeParte,
  fijarFiguras,
  fijarFlag,
  fijarSalidas,
  fijarVariable,
  figurasDe,
  NOMBRE_DE_MACRO,
  nombreDeExportacion,
  quitarVariable,
  salidasDe,
  usoDeFlag,
  type ConfigJson,
} from '../store/proyectoConfig';
import { ICONO_DE_NIVEL } from '../store/historial';

const ROL: Record<string, string> = { principal: 'Documento', auxiliar: 'Raíz auxiliar', otra: 'Incluida por otro' };
const USO: Record<UsoDeFlag, string> = { partial: 'Compilación reducida', externalize: 'Externalizar figuras', figureList: 'Listar figuras (pgf)', ninguno: 'No usar' };
const nombreCorto = (n: string) => n.split('/').pop() ?? n;
const lista = (t: string) => t.split(',').map((x) => x.trim()).filter(Boolean);

function Seccion({ titulo, ayuda, detectado, children }: { titulo: string; ayuda?: string; detectado?: () => void; children: React.ReactNode }) {
  return (
    <section className="proy-seccion">
      <header>
        <h3>{titulo}</h3>
        {detectado && (
          <button className="boton chico" onClick={detectado} title="Vuelve a lo que la app detectó para esta parte">
            Usar lo detectado
          </button>
        )}
      </header>
      {ayuda && <p className="proy-ayuda">{ayuda}</p>}
      {children}
    </section>
  );
}

function Flag({ f, config, onUso }: { f: FlagDetectado; config: ConfigJson; onUso: (u: UsoDeFlag) => void }) {
  const actual = usoDeFlag(config, f.nombre);
  return (
    <div className="proy-flag">
      <div className="proy-flag-cab">
        <code>\{f.nombre}</code>
        <select value={actual} onChange={(e) => onUso(e.target.value as UsoDeFlag)} aria-label={`Qué hace \\${f.nombre}`}>
          {(Object.keys(USO) as UsoDeFlag[]).map((u) => (
            <option key={u} value={u}>
              {USO[u]}
              {u === f.uso && u !== 'ninguno' ? ' (detectado)' : ''}
            </option>
          ))}
        </select>
      </div>
      <p className="proy-ayuda">{f.frase}</p>
    </div>
  );
}

function Probar() {
  const propuesta = useProyecto((s) => s.propuesta);
  const probar = useProyecto((s) => s.probar);
  const probando = useProyecto((s) => s.probando);
  const probada = useProyecto((s) => s.probada);
  const error = useProyecto((s) => s.errorDeProbar);
  const [parte, setParte] = useState<string>('');
  const partes = propuesta?.partes ?? [];
  const problemas = probada?.doctor.hallazgos.filter((h) => h.nivel === 'error' || h.nivel === 'aviso') ?? [];
  return (
    <Seccion titulo="Probar" ayuda="Corre el doctor y lista las figuras de una parte con esta configuración, sin guardarla. Tarda unos segundos.">
      <div className="proy-fila">
        {partes.length > 0 && (
          <select value={parte} onChange={(e) => setParte(e.target.value)} aria-label="Parte a listar">
            <option value="">la primera parte</option>
            {partes.map((p) => (
              <option key={p} value={p}>
                {nombreCorto(p)}
              </option>
            ))}
          </select>
        )}
        <button className="boton" disabled={probando} onClick={() => void probar(parte || undefined)}>
          {probando ? 'Probando…' : 'Probar'}
        </button>
      </div>
      {error && <p className="proy-error">{error}</p>}
      {probada && (
        <div className="proy-resultado">
          <div>
            Doctor: {probada.doctor.distribucion ?? 'TeX'} · {problemas.length === 0 ? 'sin errores ni avisos' : `${problemas.length} para mirar`} · {duracion(probada.doctor.ms)}
          </div>
          {problemas.map((h) => (
            <div key={`${h.id}:${h.titulo}`} className={`hallazgo ${h.nivel}`}>
              <span className="icono">{ICONO_DE_NIVEL[h.nivel]}</span> {h.titulo}
            </div>
          ))}
          <div>
            {probada.listado.omitido
              ? `Listado: ${probada.listado.omitido}.`
              : `Listado de «${nombreCorto(probada.listado.parte ?? 'todo el documento')}»: ${probada.listado.figuras} figura${probada.listado.figuras === 1 ? '' : 's'} en ${duracion(probada.listado.ms)}${probada.listado.nombres.length ? ` (${probada.listado.nombres.slice(0, 3).map(nombreCorto).join(', ')}…)` : ''}.`}
          </div>
        </div>
      )}
    </Seccion>
  );
}

export function ProyectoPantalla() {
  const s = useProyecto();
  const [varNueva, setVarNueva] = useState('');
  const [otras, setOtras] = useState(false);
  const config = s.config;
  const p = s.propuesta;

  useEffect(() => {
    if (!s.abierta) return;
    const tecla = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        if (s.paso === 'confirmar') s.volverAEditar();
        else s.cerrar();
      }
    };
    window.addEventListener('keydown', tecla, true);
    return () => window.removeEventListener('keydown', tecla, true);
  }, [s.abierta, s.paso, s]);

  const textoJson = useMemo(() => (config ? aTexto(config) : ''), [config]);
  const [borrador, setBorrador] = useState<string | null>(null);
  useEffect(() => setBorrador(null), [textoJson]);
  if (!s.abierta) return null;

  const salidas = config ? salidasDe(config) : null;
  const figuras = config ? figurasDe(config) : null;
  const flagsConUso = p?.flags.filter((f) => f.uso !== 'ninguno') ?? [];
  const flagsSinUso = p?.flags.filter((f) => f.uso === 'ninguno') ?? [];
  const ejemplo = (() => {
    if (!salidas || !p || p.partes.length === 0) return null;
    return `${salidas.exportar.replace(/^(\.\.\/)+/, '') || 'export'}/${nombreDeExportacion(salidas.patron, '1', nombreCorto(p.partes[0]))}`;
  })();
  const valida = s.validacion?.ok === true;
  const dif = s.diff;
  const resumen = resumenDeDiff(dif);

  return (
    <div className="proyecto-pantalla" role="dialog" aria-label="Proyecto" aria-modal="true">
      <div className="proy-cabecera">
        <h2>Proyecto{p ? ` · ${p.raiz}` : ''}</h2>
        <span className="proy-suave">
          {s.alAbrir ? 'Primera vez con este proyecto: revisá las formas de compilarlo y guardá.' : 'Las formas de compilar este proyecto.'}
        </span>
        <button className="boton chico" onClick={() => s.cerrar()} title="Cerrar sin guardar (Esc)">
          ✕ Cerrar
        </button>
      </div>

      {s.cargando && <div className="proy-cuerpo">Mirando el proyecto…</div>}
      {s.error && <div className="proy-error proy-cuerpo">{s.error}</div>}

      {config && p && s.paso === 'editar' && (
        <div className="proy-cuerpo">
          {/* La pantalla de perfiles es lo único que hace falta tocar. Lo que la app detecta sola queda cerrado abajo. */}
          <PerfilesEditor config={config} />

          <details className="proy-detalles">
            <summary>Configuración técnica (la detecta la app: documento principal, capítulos, macros, figuras, variables)</summary>
          {p.avisos.map((a) => (
            <div key={a} className="hallazgo aviso">
              <span className="icono">{ICONO_DE_NIVEL.aviso}</span> {a}
            </div>
          ))}

          <Seccion
            titulo="Documento principal"
            ayuda="El archivo que se compila. Lo recomendado es el que ningún otro archivo incluye y que no recorta con \includeonly; los que recortan o definen flags son raíces auxiliares (versiones reducidas para trabajar)."
            detectado={() => s.usarDetectado('raiz')}
          >
            <div className="proy-candidatas" role="radiogroup" aria-label="Documento principal">
              {p.candidatas.map((c) => (
                <label key={c.archivo} className={`proy-candidata ${config.root === c.archivo ? 'elegida' : ''}`}>
                  <input type="radio" name="raiz" checked={config.root === c.archivo} onChange={() => void s.elegirRaiz(c.archivo)} />
                  <span className="proy-candidata-cuerpo">
                    <span>
                      <strong>{c.archivo}</strong> <span className={`proy-rol ${c.rol}`}>{ROL[c.rol]}</span>
                      {c.recomendada && <span className="proy-rol recomendada">recomendada</span>}
                    </span>
                    <span className="proy-ayuda">{c.motivo}</span>
                  </span>
                </label>
              ))}
              {p.candidatas.length === 0 && <p className="proy-error">No encontré ningún archivo .tex con \documentclass.</p>}
            </div>
          </Seccion>

          <Seccion
            titulo="Partes"
            ayuda="Los capítulos que se pueden compilar de a uno. Son los que entran por \include; si el proyecto tiene una macro propia que lo envuelve (como \Capitulo), también cuenta."
            detectado={() => s.usarDetectado('partes')}
          >
            <p>
              {p.partes.length === 0 ? 'No encontré partes: el proyecto se compila entero.' : `${p.partes.length} parte${p.partes.length === 1 ? '' : 's'}: ${p.partes.slice(0, 8).map(nombreCorto).join(', ')}${p.partes.length > 8 ? ` y ${p.partes.length - 8} más` : ''}.`}
            </p>
            <label className="proy-fila">
              Comandos que delimitan una parte
              <input value={comandosDeParte(config).join(', ')} onChange={(e) => s.editar((c) => fijarComandosDeParte(c, lista(e.target.value)))} aria-label="Comandos de parte" />
            </label>
            {p.partsCommands.length > 1 && <p className="proy-ayuda">Detecté que {p.partsCommands.slice(1).map((c) => `\\${c}`).join(', ')} envuelve a \include.</p>}
          </Seccion>

          <Seccion
            titulo="Compilación reducida"
            ayuda="Macros que el proyecto usa en un \ifdefined para armar una versión más rápida (sin portada ni índices) o con las figuras en el cache. La app las define según el perfil."
            detectado={() => s.usarDetectado('flags')}
          >
            {flagsConUso.length === 0 && <p>No encontré macros de ese tipo: no hay compilación reducida que configurar.</p>}
            {flagsConUso.map((f) => (
              <Flag key={f.nombre} f={f} config={config} onUso={(u) => s.editar((c) => fijarFlag(c, f.nombre, u))} />
            ))}
            {flagsSinUso.length > 0 && (
              <>
                <button className="boton chico" onClick={() => setOtras(!otras)}>
                  {otras ? 'Ocultar' : 'Ver'} otras condiciones del proyecto ({flagsSinUso.length})
                </button>
                {otras && flagsSinUso.map((f) => <Flag key={f.nombre} f={f} config={config} onUso={(u) => s.editar((c) => fijarFlag(c, f.nombre, u))} />)}
              </>
            )}
          </Seccion>

          <Seccion titulo="Figuras" ayuda={p.figuras.motivo} detectado={() => s.usarDetectado('figuras')}>
            {figuras && (
              <div className="proy-fila proy-fila-ancha">
                <label>
                  Modo
                  <select value={figuras.modo} onChange={(e) => s.editar((c) => fijarFiguras(c, { ...figuras, modo: e.target.value as 'project' | 'app' | 'off' }))}>
                    <option value="project">project: el cache que ya usa el proyecto</option>
                    <option value="app">app: la app externaliza por vos</option>
                    <option value="off">off: sin cache de figuras</option>
                  </select>
                </label>
                {figuras.modo !== 'off' && (
                  <>
                    <label>
                      Carpeta del cache
                      <input value={figuras.dir} onChange={(e) => s.editar((c) => fijarFiguras(c, { ...figuras, dir: e.target.value }))} />
                    </label>
                    <label title="El archivo cuyo contenido cambia el aspecto de las figuras: su md5 es el sello del cache">
                      Archivo de estilo (sello)
                      <input value={figuras.stampFiles.join(', ')} onChange={(e) => s.editar((c) => fijarFiguras(c, { ...figuras, stampFiles: lista(e.target.value) }))} />
                    </label>
                  </>
                )}
              </div>
            )}
          </Seccion>

          <Seccion titulo="Salidas" ayuda="Dónde van el PDF que se publica y los PDF de cada capítulo exportado." detectado={() => s.usarDetectado('salidas')}>
            {salidas && (
              <div className="proy-fila proy-fila-ancha">
                <label>
                  Publicar en
                  <input value={salidas.publicar} placeholder="(no se publica)" onChange={(e) => s.editar((c) => fijarSalidas(c, { ...salidas, publicar: e.target.value }))} />
                </label>
                <label>
                  Exportar capítulos en
                  <input value={salidas.exportar} placeholder="export" onChange={(e) => s.editar((c) => fijarSalidas(c, { ...salidas, exportar: e.target.value }))} />
                </label>
                <label title="{num} es el número de capítulo impreso, {num:02} lo completa con ceros y {name} es el nombre de la parte">
                  Nombre
                  <input value={salidas.patron} onChange={(e) => s.editar((c) => fijarSalidas(c, { ...salidas, patron: e.target.value }))} />
                </label>
              </div>
            )}
            {ejemplo && <p className="proy-ayuda">Un capítulo exportado quedaría como «{ejemplo}».</p>}
          </Seccion>

          <Seccion titulo="Variables" ayuda="Valores que el menú de compilación (Ctrl+Alt+T) cambia sin tocar el proyecto; la app los define antes de cargar el documento.">
            {Object.entries(config.variables ?? {}).map(([nombre, v]) => (
              <div key={nombre} className="proy-variable">
                <code>\{nombre}</code>
                <input value={v.label ?? ''} placeholder="Etiqueta" onChange={(e) => s.editar((c) => fijarVariable(c, nombre, { ...v, label: e.target.value }))} aria-label={`Etiqueta de ${nombre}`} />
                <input
                  value={(v.choices ?? []).join(', ')}
                  placeholder="opciones separadas por coma"
                  onChange={(e) => s.editar((c) => fijarVariable(c, nombre, { ...v, choices: lista(e.target.value) }))}
                  aria-label={`Opciones de ${nombre}`}
                />
                <select value={v.default ?? ''} onChange={(e) => s.editar((c) => fijarVariable(c, nombre, { ...v, default: e.target.value || undefined }))} aria-label={`Valor por omisión de ${nombre}`}>
                  <option value="">(sin valor)</option>
                  {(v.choices ?? []).map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
                <button className="boton chico peligro" onClick={() => s.editar((c) => quitarVariable(c, nombre))} title="Quitar la variable">
                  ✕
                </button>
              </div>
            ))}
            {p.variables
              .filter((v) => !(v.nombre in (config.variables ?? {})))
              .map((v) => (
                <div key={v.nombre} className="proy-sugerida">
                  <p className="proy-ayuda">{v.frase}</p>
                  <button
                    className="boton chico"
                    onClick={() => s.editar((c) => fijarVariable(c, v.nombre, { label: v.label, choices: v.choices, default: v.default }))}
                  >
                    Agregar \{v.nombre}
                  </button>
                </div>
              ))}
            {p.variables
              .filter((v) => v.nombre in (config.variables ?? {}) && v.defSinGuardia.length > 0)
              .map((v) => (
                <div key={`g:${v.nombre}`} className="hallazgo aviso">
                  <span className="icono">{ICONO_DE_NIVEL.aviso}</span> {v.frase}
                </div>
              ))}
            <div className="proy-fila">
              <input placeholder="macro de la variable (sólo letras)" value={varNueva} onChange={(e) => setVarNueva(e.target.value)} aria-label="Nombre de la variable nueva" />
              <button
                className="boton chico"
                disabled={!NOMBRE_DE_MACRO.test(varNueva) || varNueva in (config.variables ?? {})}
                onClick={() => {
                  s.editar((c) => fijarVariable(c, varNueva, { label: varNueva, choices: [] }));
                  setVarNueva('');
                }}
              >
                Agregar variable
              </button>
            </div>
          </Seccion>

          <Seccion titulo="Avanzado" ayuda="El latexviewer.json completo, para quien prefiera editarlo. Se valida al escribir; lo que no se entiende se conserva.">
            <textarea
              className="proy-json"
              spellCheck={false}
              value={borrador ?? textoJson}
              onChange={(e) => {
                setBorrador(e.target.value);
                s.editarJson(e.target.value);
              }}
              aria-label="latexviewer.json"
            />
            {s.errorDeJson && <p className="proy-error">{s.errorDeJson}</p>}
          </Seccion>

          <Probar />
          </details>
        </div>
      )}

      {config && s.paso === 'confirmar' && (
        <div className="proy-cuerpo">
          <h3>Lo que va a cambiar en latexviewer.json</h3>
          <p className="proy-ayuda">
            {s.textoDisco === null ? 'El archivo no existe: se crea.' : `${resumen.agregadas} línea${resumen.agregadas === 1 ? '' : 's'} nueva${resumen.agregadas === 1 ? '' : 's'} y ${resumen.quitadas} que se van.`}
          </p>
          <pre className="proy-diff" aria-label="Cambios">
            {diffCompacto(dif, 3).map((l, i) => (
              <div key={i} className={`diff-${l.tipo}`}>
                {l.tipo === 'agrega' ? '+ ' : l.tipo === 'quita' ? '- ' : '  '}
                {l.texto}
              </div>
            ))}
          </pre>
        </div>
      )}

      <div className="proy-pie">
        {config && s.paso === 'editar' && (
          <>
            <span className={`proy-estado ${valida ? 'ok' : 'mal'}`}>
              {s.validacion === null ? 'Revisando…' : valida ? '✓ La configuración es válida' : `✘ ${s.validacion.errores[0] ?? 'La configuración no es válida'}`}
            </span>
            <button className="boton primario" disabled={!valida} onClick={() => void s.pedirGuardar()}>
              {s.guardando ? 'Guardando…' : 'Guardar'}
            </button>
          </>
        )}
        {s.paso === 'confirmar' && (
          <>
            <button className="boton" onClick={() => s.volverAEditar()}>
              ← Volver
            </button>
            <button className="boton primario" disabled={s.guardando} onClick={() => void s.confirmarGuardado()}>
              {s.guardando ? 'Escribiendo…' : 'Confirmar y escribir latexviewer.json'}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
