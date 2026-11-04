// La pestaña «En vivo» del panel PDF (la página que TeXpresso ya tiene dibujada, mandada como imagen): la parte pura, sin DOM.

/** Lo más angosta y lo más ancha que se le pide la imagen a TeXpresso: más ancha cuesta dibujar, codificar y cruzar `wsl.exe` de más. */
export const ANCHO_MIN = 480;
export const ANCHO_MAX = 1800;

/**
 * El ancho en píxeles que se le pide a TeXpresso para un panel de `anchoPanel` píxeles CSS: el ancho del panel por la densidad de
 * píxeles de la pantalla (para que el texto se vea nítido), redondeado a múltiplos de 40 para no pedir uno nuevo con cada píxel que
 * se mueva la barra separadora.
 */
export function anchoPedido(anchoPanel: number, dpr: number): number {
  const px = Math.round((anchoPanel * (dpr > 0 ? dpr : 1)) / 40) * 40;
  return Math.min(ANCHO_MAX, Math.max(ANCHO_MIN, px));
}

/**
 * Un clic en la imagen mostrada (`offsetX/offsetY` en píxeles CSS, dentro de un elemento de `mostradoW × mostradoH`) en las
 * coordenadas de la imagen que mandó TeXpresso (`ancho × alto` píxeles): es lo que entiende `live-click`.
 */
export function puntoDeClic(offsetX: number, offsetY: number, mostradoW: number, mostradoH: number, ancho: number, alto: number): { x: number; y: number } | null {
  if (mostradoW <= 0 || mostradoH <= 0 || ancho <= 0 || alto <= 0) return null;
  const x = (offsetX / mostradoW) * ancho;
  const y = (offsetY / mostradoH) * alto;
  if (x < 0 || y < 0 || x > ancho || y > alto) return null;
  return { x: Math.round(x * 10) / 10, y: Math.round(y * 10) / 10 };
}

/**
 * «pág. 3» (desde 1); vacío si todavía no llegó ninguna imagen. No lleva el total: TeXpresso compila sólo hasta la página que muestra,
 * así que las `paginas` que informa son las compiladas hasta ahora (la página mostrada, casi siempre), no las del documento.
 */
export function textoPagina(pagina: number | null): string {
  if (pagina === null) return '';
  return `pág. ${pagina + 1}`;
}
