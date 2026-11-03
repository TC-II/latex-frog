// Estado de la pantalla «Proyecto» (F8b). Es un store aparte del de la app: la pantalla se abre, se edita una copia de la configuración y
// sólo al confirmar se escribe `latexviewer.json`. Todo lo que se detecta y se valida viene del core por el `Backend`; acá no hay E/S.
import { create } from 'zustand';
import { getBackend, type ProjectInfo, type Propuesta, type ResultadoProbar, type ValidacionConfig } from '../backend';
import { diffDeLineas, type LineaDeDiff } from './diff';
import { aplicarSeccion, aTexto, fijarRaiz, parsear, type ConfigJson, type SeccionDeConfig } from './proyectoConfig';

/** Lo que la app hace con la vista del proyecto que devuelve `writeConfig` (la registra `store/app.ts` al arrancar). */
let alGuardar: (info: ProjectInfo) => void = () => {};
export const setAlGuardar = (f: (info: ProjectInfo) => void) => {
  alGuardar = f;
};

export type PasoDePantalla = 'editar' | 'confirmar';

const mensajeDe = (e: unknown) => (e instanceof Error ? e.message : String(e));

export interface ProyectoState {
  abierta: boolean;
  /** Se abrió sola porque el proyecto no tiene `latexviewer.json`. */
  alAbrir: boolean;
  cargando: boolean;
  error: string | null;
  propuesta: Propuesta | null;
  /** La propuesta como objeto (para «usar lo detectado» de cada sección). */
  detectada: ConfigJson | null;
  /** Lo que hay en el disco; `null` si el proyecto no tiene `latexviewer.json`. */
  textoDisco: string | null;
  /** La copia que se edita. */
  config: ConfigJson | null;
  validacion: ValidacionConfig | null;
  paso: PasoDePantalla;
  diff: LineaDeDiff[];
  guardando: boolean;
  probando: boolean;
  probada: ResultadoProbar | null;
  errorDeProbar: string | null;
  /** El JSON que se está escribiendo en «Avanzado» cuando todavía no se puede leer. */
  errorDeJson: string | null;

  abrir(opciones?: { auto?: boolean; raiz?: string }): Promise<void>;
  cerrar(): void;
  elegirRaiz(rel: string): Promise<void>;
  editar(f: (c: ConfigJson) => ConfigJson): void;
  usarDetectado(seccion: SeccionDeConfig): void;
  editarJson(texto: string): void;
  pedirGuardar(): Promise<void>;
  volverAEditar(): void;
  confirmarGuardado(): Promise<boolean>;
  probar(parte?: string): Promise<void>;
}

export const PROYECTO_INICIAL = {
  abierta: false,
  alAbrir: false,
  cargando: false,
  error: null,
  propuesta: null,
  detectada: null,
  textoDisco: null,
  config: null,
  validacion: null,
  paso: 'editar' as PasoDePantalla,
  diff: [] as LineaDeDiff[],
  guardando: false,
  probando: false,
  probada: null,
  errorDeProbar: null,
  errorDeJson: null,
};

/** Para que una validación lenta no pise a una más nueva. */
let contadorDeValidacion = 0;

export const useProyecto = create<ProyectoState>()((set, get) => {
  const validar = async (config: ConfigJson): Promise<ValidacionConfig | null> => {
    const n = ++contadorDeValidacion;
    try {
      const v = await getBackend().validateConfig(aTexto(config));
      if (n === contadorDeValidacion) set({ validacion: v });
      return v;
    } catch (e) {
      if (n === contadorDeValidacion) set({ validacion: { ok: false, errores: [mensajeDe(e)], normalizado: null, ejemploNombre: null } });
      return null;
    }
  };

  return {
    ...PROYECTO_INICIAL,

    async abrir(opciones) {
      set({ ...PROYECTO_INICIAL, abierta: true, alAbrir: opciones?.auto === true, cargando: true });
      const b = getBackend();
      try {
        const [propuesta, disco] = await Promise.all([b.detectProject(opciones?.raiz), b.configOnDisk()]);
        const detectada = parsear(propuesta.texto);
        // Si hay un `latexviewer.json` legible, se parte de él (la detección sólo sugiere); si no, de lo detectado.
        const delDisco = disco !== null ? parsear(disco) : null;
        let config = delDisco ?? detectada;
        let error: string | null = null;
        if (disco !== null && delDisco === null) error = 'El latexviewer.json que hay no se pudo leer: se parte de lo detectado y, al guardar, se reemplaza.';
        if (config && opciones?.raiz) config = fijarRaiz(config, opciones.raiz);
        set({ propuesta, detectada, textoDisco: disco, config, cargando: false, error });
        if (config) void validar(config);
      } catch (e) {
        set({ cargando: false, error: mensajeDe(e) });
      }
    },

    cerrar() {
      contadorDeValidacion++;
      set({ ...PROYECTO_INICIAL });
    },

    async elegirRaiz(rel) {
      const { config, textoDisco } = get();
      try {
        const propuesta = await getBackend().detectProject(rel);
        const detectada = parsear(propuesta.texto);
        // Sin archivo en el disco todo sale de la detección y cambia con la raíz; con uno, sólo la raíz (lo demás es de quien lo editó).
        const nueva = textoDisco === null && detectada ? detectada : config ? fijarRaiz(config, rel) : detectada;
        set({ propuesta, detectada, config: nueva, probada: null, errorDeProbar: null });
        if (nueva) void validar(nueva);
      } catch (e) {
        set({ error: mensajeDe(e) });
      }
    },

    editar(f) {
      const c = get().config;
      if (!c) return;
      const nueva = f(c);
      set({ config: nueva, errorDeJson: null, paso: 'editar' });
      void validar(nueva);
    },

    usarDetectado(seccion) {
      const { config, detectada } = get();
      if (!config || !detectada) return;
      get().editar((c) => aplicarSeccion(c, detectada, seccion));
    },

    editarJson(texto) {
      const c = parsear(texto);
      if (!c) {
        set({ errorDeJson: 'El JSON no se puede leer todavía (falta una coma, una llave o una comilla).' });
        return;
      }
      get().editar(() => c);
    },

    async pedirGuardar() {
      const { config } = get();
      if (!config) return;
      const v = await validar(config);
      if (!v?.ok || v.normalizado === null) return;
      // Se guarda y listo: mostrar el diff del JSON antes de escribir era un paso que nadie quería ver.
      set({ validacion: v, diff: diffDeLineas(get().textoDisco, v.normalizado), error: null });
      await get().confirmarGuardado();
    },

    volverAEditar() {
      set({ paso: 'editar' });
    },

    async confirmarGuardado() {
      const v = get().validacion;
      if (!v?.ok || v.normalizado === null) return false;
      set({ guardando: true, error: null });
      try {
        const info = await getBackend().writeConfig(v.normalizado);
        alGuardar(info);
        set({ ...PROYECTO_INICIAL });
        return true;
      } catch (e) {
        set({ guardando: false, error: mensajeDe(e), paso: 'editar' });
        return false;
      }
    },

    async probar(parte) {
      const { config } = get();
      if (!config) return;
      set({ probando: true, probada: null, errorDeProbar: null });
      try {
        set({ probada: await getBackend().probeConfig(aTexto(config), parte), probando: false });
      } catch (e) {
        set({ probando: false, errorDeProbar: mensajeDe(e) });
      }
    },
  };
});

/** Abre la pantalla (desde la app: al abrir un proyecto sin `latexviewer.json`, el menú ⋯ y la paleta). */
export const abrirPantallaProyecto = (opciones?: { auto?: boolean; raiz?: string }) => useProyecto.getState().abrir(opciones);
