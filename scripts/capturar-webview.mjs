// Captura del contenido de la ventana de la app (el WebView2), sin tocar la pantalla ni el mouse: pide la imagen al propio navegador
// por el protocolo de depuración de Chromium (CDP). La captura toma lo que el WebView dibuja, no lo que haya encima en el escritorio
// (CLAUDE.md: nada de capturas de la pantalla ni de mover el mouse para probar la interfaz).
//
// Lo lanza `scripts/aceptacion-texpresso.ps1 -Captura` en segundo plano, con la app abierta con
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=<puerto> --remote-allow-origins=*". El smoke (`LV_SMOKE=texpresso`)
// deja `window.__capturaLista = true` cuando la pestaña «En vivo» tiene la página a la vista; este script la espera, saca la
// captura, la guarda y le avisa al smoke con `window.__capturaHecha = true`.
//
// Uso: node scripts/capturar-webview.mjs <puerto> <salida.png> [limite-en-segundos]
import { writeFileSync } from 'node:fs';

const [puerto, salida, limite = '900'] = process.argv.slice(2);
if (!puerto || !salida) {
  console.error('uso: node capturar-webview.mjs <puerto> <salida.png> [limite-en-segundos]');
  process.exit(2);
}
const fin = Date.now() + Number(limite) * 1000;
const dormir = (ms) => new Promise((r) => setTimeout(r, ms));

/** El objetivo `page` de la app (el WebView2 sólo tiene una página). */
async function buscarPagina() {
  while (Date.now() < fin) {
    try {
      const lista = await (await fetch(`http://127.0.0.1:${puerto}/json/list`)).json();
      const p = lista.find((t) => t.type === 'page' && t.webSocketDebuggerUrl);
      if (p) return p;
    } catch {
      // La app todavía no abrió el puerto.
    }
    await dormir(500);
  }
  throw new Error('no apareció la página de la app en el puerto de depuración');
}

const pagina = await buscarPagina();
const ws = new WebSocket(pagina.webSocketDebuggerUrl);
await new Promise((res, rej) => {
  ws.onopen = res;
  ws.onerror = () => rej(new Error('no se pudo abrir el websocket de depuración'));
});
// Si la app se cierra antes de pedir la captura (el smoke falló), no se queda esperando una respuesta que no va a llegar.
ws.onclose = () => {
  console.error('la app cerró la conexión sin que el smoke pidiera la captura');
  process.exit(1);
};
let id = 0;
const pendientes = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pendientes.has(d.id)) {
    pendientes.get(d.id)(d);
    pendientes.delete(d.id);
  }
};
const llamar = (method, params = {}) =>
  new Promise((res) => {
    const mi = ++id;
    pendientes.set(mi, res);
    ws.send(JSON.stringify({ id: mi, method, params }));
  });
const evaluar = async (expresion) => (await llamar('Runtime.evaluate', { expression: expresion, returnByValue: true })).result?.result?.value;

try {
  while (Date.now() < fin) {
    if ((await evaluar('window.__capturaLista === true')) === true) {
      const r = await llamar('Page.captureScreenshot', { format: 'png' });
      if (!r.result?.data) throw new Error(`la captura falló: ${JSON.stringify(r.error ?? r)}`);
      writeFileSync(salida, Buffer.from(r.result.data, 'base64'));
      await evaluar('window.__capturaHecha = true');
      console.log(`captura guardada en ${salida}`);
      break;
    }
    await dormir(300);
  }
  if (Date.now() >= fin) throw new Error('se acabó el tiempo sin que el smoke pidiera la captura');
} finally {
  ws.close();
}
process.exit(0);
