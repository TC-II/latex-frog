import { describe, expect, it } from 'vitest';
import { ANCHO_MAX, ANCHO_MIN, anchoPedido, puntoDeClic, textoPagina } from './vivo';

describe('la pestaña En vivo', () => {
  it('pide un ancho acotado y en múltiplos de 40', () => {
    expect(anchoPedido(700, 1)).toBe(720);
    expect(anchoPedido(700, 1.25)).toBe(880);
    expect(anchoPedido(100, 1)).toBe(ANCHO_MIN);
    expect(anchoPedido(4000, 2)).toBe(ANCHO_MAX);
    // Una densidad rara no rompe el cálculo.
    expect(anchoPedido(700, 0)).toBe(720);
    // Mover la barra unos píxeles no cambia el pedido.
    expect(anchoPedido(701, 1)).toBe(anchoPedido(705, 1));
  });

  it('lleva un clic a las coordenadas de la imagen de TeXpresso', () => {
    // La imagen de 800×1036 se muestra a 400×518: el clic en (200, 259) cae en el centro.
    expect(puntoDeClic(200, 259, 400, 518, 800, 1036)).toEqual({ x: 400, y: 518 });
    expect(puntoDeClic(0, 0, 400, 518, 800, 1036)).toEqual({ x: 0, y: 0 });
    // Fuera de la imagen o sin tamaño: nada.
    expect(puntoDeClic(500, 10, 400, 518, 800, 1036)).toBeNull();
    expect(puntoDeClic(-1, 10, 400, 518, 800, 1036)).toBeNull();
    expect(puntoDeClic(10, 10, 0, 518, 800, 1036)).toBeNull();
  });

  it('cuenta las páginas desde 1', () => {
    expect(textoPagina(0)).toBe('pág. 1');
    expect(textoPagina(10)).toBe('pág. 11');
    expect(textoPagina(null)).toBe('');
  });
});
