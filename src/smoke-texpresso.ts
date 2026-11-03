// Smoke de aceptación de la vista previa con TeXpresso (`LV_SMOKE=texpresso`). Corre sola, dentro de la app real, sobre una copia del
// libro, con TeXpresso de verdad en WSL (`scripts/aceptacion-texpresso.ps1`):
//   - el interruptor está en la barra y, apagado, no hay driver de TeXpresso ni procesos en la distro,
//   - un clic lo prende (iniciando → listo) y queda guardado en `.lv/state.json`,
//   - cada cambio tecleado en el editor llega a TeXpresso en milisegundos (sin guardar el archivo),
//   - un error de TeX aparece en la barra y se va al corregirlo,
//   - un cuelgue (SIGSTOP) se detecta por el ping y se reinicia solo,
//   - latexmk sigue compilando igual con TeXpresso prendido, y su driver no es el de TeXpresso,
//   - un clic lo apaga y no quedan procesos en la distro.
// Con `LV_SMOKE_FILE=reabrir` (segunda corrida del script): al abrir el proyecto con el interruptor guardado prendido, se lanza solo.
import { getBackend, type LaunchOptions } from './backend';
import { aplicarEnVista, contenidoDeVista, escribirComoTecleado, getDoc, moverCursor } from './editor/docs';
import { iguales } from './smoke-util';
import { dormir, esperarHasta } from './smoke-util';
import { useApp } from './store/app';
import { useTexpresso } from './store/texpresso';

const CAPITULO = 'Secciones/Aproximación de Butterworth.tex';
const MARCA = 'Esta es la función de aproximación de';
const CAMBIOS = 15;

interface Paso {
  nombre: string;
  ok: boolean;
  ms: number;
  detalle?: string;
}

const mediana = (v: number[]) => {
  const o = [...v].sort((a, b) => a - b);
  return o.length ? o[Math.floor(o.length / 2)] : NaN;
};

export async function correrSmokeTexpresso(opts: LaunchOptions): Promise<void> {
  const backend = getBackend();
  const store = () => useApp.getState();
  const tp = () => useTexpresso.getState();
  const fase = () => tp().vista?.fase ?? 'apagado';
  const pasos: Paso[] = [];
  const datos: Record<string, unknown> = {};
  let fallo: string | null = null;
  let nota: string | undefined;
  const detalle = (t: string) => {
    nota = t;
  };
  const paso = async <T>(nombre: string, fn: () => Promise<T>): Promise<T> => {
    const t0 = Date.now();
    nota = undefined;
    try {
      const r = await fn();
      pasos.push({ nombre, ok: true, ms: Date.now() - t0, detalle: nota });
      return r;
    } catch (e) {
      pasos.push({ nombre, ok: false, ms: Date.now() - t0, detalle: e instanceof Error ? e.message : String(e) });
      throw e;
    }
  };
  const boton = () => document.querySelector<HTMLButtonElement>('.barra-superior .texpresso-boton');
  const estadoGuardado = async () => JSON.parse((await backend.readFile('.lv/state.json')).text).texpresso === true;
  const existe = async (rel: string) => {
    try {
      await backend.readFile(rel);
      return true;
    } catch {
      return false;
    }
  };
  const procesos = () => backend.smokeTexpressoProcesos!();

  /** Abre el capítulo, deja el cursor en el párrafo de la marca y espera a que TeXpresso esté mostrando esa página. */
  const irAlParrafo = async () => {
    await store().abrirArchivo(CAPITULO);
    await esperarHasta(() => contenidoDeVista() !== null && getDoc(CAPITULO) !== undefined, 15_000, 'el editor con el capítulo');
    const texto = getDoc(CAPITULO)!.state.doc.toString();
    const i = texto.indexOf(MARCA);
    if (i < 0) throw new Error('no está la marca en el capítulo');
    moverCursor(CAPITULO, i + MARCA.length);
    await dormir(800);
    return i + MARCA.length;
  };

  /**
   * Teclea de a un carácter y, antes del siguiente, espera a que TeXpresso muestre ese cambio (hasta 3 s): lo que tardó cada uno.
   * Teclear más rápido de lo que TeXpresso recompila no da una salida por tecla: cada cambio nuevo lo hace volver atrás antes de terminar
   * el anterior, y la salida llega cuando se deja de tipear (eso lo mide `rafaga`).
   */
  const teclear = async (texto: string) => {
    const antes = tp().tiempos.length;
    for (const c of texto) {
      const n = tp().tiempos.length;
      escribirComoTecleado(c);
      const t0 = Date.now();
      while (tp().tiempos.length === n && Date.now() - t0 < 3000) await dormir(10);
      await dormir(100);
    }
    return tp().tiempos.slice(antes);
  };

  /** Teclea `texto` de corrido (una tecla cada `cadaMs`) y devuelve lo que tardó TeXpresso en mostrar el último cambio. */
  const rafaga = async (texto: string, cadaMs: number) => {
    const n = tp().tiempos.length;
    for (const c of texto) {
      escribirComoTecleado(c);
      await dormir(cadaMs);
    }
    const t0 = Date.now();
    while (Date.now() - t0 < 5000) {
      await dormir(20);
      // La salida del último: llega cuando TeXpresso termina de recompilar (sin otra tecla en camino).
      if (tp().tiempos.length > n && Date.now() - t0 > 400) break;
    }
    return tp().tiempos.slice(n);
  };

  const trabajo = (async () => {
    await esperarHasta(() => store().project !== null && boton() !== null && tp().disp !== null, 60_000, 'el proyecto y el interruptor');

    if (opts.smokeFile === 'reabrir') {
      await paso('con el interruptor guardado prendido, abrir el proyecto lanza TeXpresso solo', async () => {
        const t0 = Date.now();
        await esperarHasta(() => fase() === 'listo', 120_000, 'TeXpresso listo al abrir').catch((e) => {
          throw new Error(`${e.message} (aviso: ${store().notice?.texto ?? 'ninguno'}; vista: ${JSON.stringify(tp().vista)})`);
        });
        datos.reabrirMs = Date.now() - t0;
        if (!(await estadoGuardado())) throw new Error('state.json no dice texpresso: true');
        const pos = await irAlParrafo();
        const t = await teclear(' x');
        if (t.length === 0) throw new Error('los cambios no le llegaron');
        moverCursor(CAPITULO, pos);
        detalle(`listo ${datos.reabrirMs} ms después de abrir; ${t.length} cambios (${t.join(', ')} ms)`);
      });
      return;
    }

    await paso('el interruptor está en la barra, apagado, sin driver ni procesos de TeXpresso', async () => {
      const b = boton()!;
      if (b.disabled) throw new Error(`deshabilitado: ${b.title}`);
      if (fase() !== 'apagado') throw new Error(`fase ${fase()}`);
      if (await existe('.lv/drivers/lv-texpresso.tex')) throw new Error('hay driver de TeXpresso sin haberlo prendido');
      if (await estadoGuardado()) throw new Error('state.json arrancó con texpresso: true');
      const n = await procesos();
      if (n !== 0) throw new Error(`${n} procesos de TeXpresso antes de prenderlo`);
      detalle(`«${b.textContent}»; WSL: ${tp().disp?.distro ?? '?'}`);
    });

    await paso('un clic lo prende: iniciando y después listo, y queda guardado en state.json', async () => {
      const t0 = Date.now();
      boton()!.click();
      const que = () => `(aviso: ${store().notice?.texto ?? 'ninguno'}; vista: ${JSON.stringify(tp().vista)}; disp: ${JSON.stringify(tp().disp)})`;
      try {
        await esperarHasta(() => fase() === 'iniciando' || fase() === 'listo', 60_000, 'iniciando');
        await esperarHasta(() => fase() === 'listo', 120_000, 'listo');
      } catch (e) {
        throw new Error(`${e instanceof Error ? e.message : e} ${que()}`);
      }
      datos.prenderMs = Date.now() - t0;
      datos.arranqueMs = tp().vista?.arranqueMs;
      if (!boton()!.textContent?.includes('listo')) throw new Error(`la barra dice «${boton()!.textContent}»`);
      if (!(await estadoGuardado())) throw new Error('state.json no dice texpresso: true');
      const driver = (await backend.readFile('.lv/drivers/lv-texpresso.tex')).text;
      if (!driver.includes('graphics if exists')) throw new Error(`el driver de TeXpresso no consume sólo el cache:\n${driver}`);
      detalle(`clic → listo en ${datos.prenderMs} ms (primer ciclo de TeXpresso: ${datos.arranqueMs} ms); ${await procesos()} procesos en la distro`);
    });

    const pos = await paso('cada cambio tecleado en el capítulo llega a TeXpresso en milisegundos, sin guardar', async () => {
      const enDisco = await backend.smokeReadBytes(CAPITULO);
      const p = await irAlParrafo();
      const t = await teclear(' (vista en vivo)'.slice(0, CAMBIOS));
      datos.cambios = t;
      if (t.length < CAMBIOS * 0.8) throw new Error(`sólo ${t.length} de ${CAMBIOS} cambios dieron salida: ${t.join(', ')}`);
      const m = mediana(t);
      datos.medianaMs = m;
      if (m > 500) throw new Error(`mediana ${m} ms`);
      // De corrido, una tecla cada 80 ms: la vista se pone al día poco después de la última.
      const r = await rafaga(' ok rapido', 80);
      datos.rafaga = r;
      const ultimo = r[r.length - 1];
      if (ultimo === undefined || ultimo > 1500) throw new Error(`después de una ráfaga, la vista tardó ${ultimo ?? '∞'} ms`);
      if (store().tabs.find((x) => x.rel === CAPITULO)?.dirty !== true) throw new Error('la pestaña no quedó modificada');
      // TeXpresso recibe el buffer: el archivo del proyecto no se tocó.
      const dif = iguales(enDisco, await backend.smokeReadBytes(CAPITULO));
      if (dif) throw new Error(`el capítulo cambió en disco sin guardar: ${dif}`);
      detalle(`${t.length}/${CAMBIOS} cambios de a uno; mediana ${m} ms, máximo ${Math.max(...t)} ms (${t.join(', ')}); ráfaga de 10 teclas: ${r.length} salida(s), la última ${ultimo} ms después de la última tecla`);
      return p;
    });

    await paso('un error de TeX aparece en la barra y se va al corregirlo', async () => {
      const ahora = getDoc(CAPITULO)!.state.selection.main.head;
      escribirComoTecleado(' \\comandoQueNoExiste ');
      await esperarHasta(() => (tp().vista?.errores ?? 0) > 0, 15_000, 'el error en la barra');
      const texto = boton()!.textContent ?? '';
      if (!texto.includes('error')) throw new Error(`la barra dice «${texto}»`);
      const titulo = boton()!.title;
      // Se borra lo tecleado.
      const fin = getDoc(CAPITULO)!.state.selection.main.head;
      if (!aplicarEnVista(() => ({ changes: { from: ahora, to: fin }, selection: { anchor: ahora } }))) throw new Error('no se pudo borrar el error');
      await esperarHasta(() => (tp().vista?.errores ?? 0) === 0, 15_000, 'que el error se vaya');
      detalle(`«${texto}»; ${titulo.split('\n').find((l) => l.startsWith('Primer error')) ?? ''}`);
    });

    await paso('un cuelgue se detecta por el ping, se reinicia solo y recibe otra vez lo que el editor tiene sin guardar', async () => {
      const t0 = Date.now();
      if (!(await backend.smokeTexpressoColgar!())) throw new Error('no se pudo simular el cuelgue');
      await esperarHasta(() => fase() === 'caido', 30_000, 'que se lo dé por caído');
      datos.detectarCuelgueMs = Date.now() - t0;
      const motivo = tp().vista?.motivo ?? '';
      if (!motivo.includes('no respondió')) throw new Error(`motivo: ${motivo}`);
      await esperarHasta(() => fase() === 'listo', 120_000, 'que vuelva a estar listo');
      datos.recuperarMs = Date.now() - t0;
      if ((tp().vista?.reinicios ?? 0) < 1) throw new Error('no contó el reinicio');
      moverCursor(CAPITULO, pos);
      await dormir(800);
      const t = await teclear('!');
      if (t.length === 0) throw new Error('después del reinicio los cambios no llegan');
      detalle(`caído a los ${datos.detectarCuelgueMs} ms («${motivo}»), listo otra vez a los ${datos.recuperarMs} ms; un cambio después: ${t.join(', ')} ms`);
    });

    await paso('latexmk sigue compilando con TeXpresso prendido, y su driver no es el de TeXpresso', async () => {
      // Lo del editor (con los cambios de prueba) se guarda en una copia del libro: es lo que haría la persona con Ctrl+S.
      await store().compilar(undefined, { guardar: true });
      await esperarHasta(() => store().build.status !== 'running', 10 * 60_000, 'el build');
      if (store().build.status !== 'ok') throw new Error(`el build terminó ${store().build.status}`);
      const perfil = store().project!.state.activeProfile!;
      const d = (await backend.readFile(`.lv/drivers/lv-${perfil}.tex`)).text;
      const dt = (await backend.readFile('.lv/drivers/lv-texpresso.tex')).text;
      if (d === dt) throw new Error('latexmk compiló el driver de TeXpresso');
      if (fase() !== 'listo') throw new Error(`TeXpresso quedó ${fase()} después del build`);
      datos.buildMs = store().build.ms;
      detalle(`build de ${perfil}: ${store().build.ms} ms, PDF ${store().pdfRel}; TeXpresso sigue listo`);
    });

    await paso('un clic lo apaga, state.json lo dice y no quedan procesos en la distro', async () => {
      boton()!.click();
      await esperarHasta(() => fase() === 'apagado', 15_000, 'apagado');
      const t0 = Date.now();
      let n = await procesos();
      while (n > 0 && Date.now() - t0 < 15_000) {
        await dormir(300);
        n = await procesos();
      }
      datos.apagarMs = Date.now() - t0;
      if (n !== 0) throw new Error(`quedaron ${n} procesos`);
      if (await estadoGuardado()) throw new Error('state.json sigue diciendo texpresso: true');
      detalle(`sin procesos a los ${datos.apagarMs} ms`);
    });

    await paso('se vuelve a prender y la app sale con TeXpresso corriendo (el script mira que no quede nada)', async () => {
      boton()!.click();
      await esperarHasta(() => fase() === 'listo', 120_000, 'listo otra vez');
      detalle(`${await procesos()} procesos al salir`);
    });
  })();

  try {
    await Promise.race([trabajo, new Promise<never>((_, r) => setTimeout(() => r(new Error('tope de tiempo')), 20 * 60_000))]);
  } catch (e) {
    fallo = e instanceof Error ? e.message : String(e);
  }
  const resultado = { ok: fallo === null && pasos.every((p) => p.ok), fallo, pasos, ...datos };
  try {
    await backend.writeSmokeResult(JSON.stringify(resultado, null, 2));
    if (opts.smokePausaMs > 0) await dormir(opts.smokePausaMs);
  } finally {
    await backend.exitApp();
  }
}
