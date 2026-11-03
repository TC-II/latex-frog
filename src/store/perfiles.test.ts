import { describe, expect, it } from 'vitest';
import type { PiezaInfo } from '../backend';
import { aPerfil, carpetaRelativa, capitulosQueEntran, filas, loQueSale, nombreDeSalida, seleccionDe } from './perfiles';

const pz = (clave: string, nombre: string, lugar: string): PiezaInfo => ({ clave, nombre, archivo: 'main.tex', linea: 1, lugar });
const PIEZAS = [pz('pdf:Tapa.pdf', 'Tapa', 'antes'), pz('toc', 'Índice', 'antes'), pz('bibliography', 'Bibliografía', 'despues'), pz('pdf:Contra.pdf', 'Contra', 'despues')];
const PARTES = ['Secciones/A', 'Secciones/B', 'Secciones/C'];

describe('perfiles como lista de piezas', () => {
  it('ordena lo de antes, los capítulos y lo de después', () => {
    const f = filas(PIEZAS, PARTES).map((x) => (x.tipo === 'pieza' ? x.clave : x.tipo === 'capitulo' ? x.parte : x.tipo));
    expect(f).toEqual(['pdf:Tapa.pdf', 'toc', 'actual', 'todos', ...PARTES, 'bibliography', 'pdf:Contra.pdf']);
    expect(filas(PIEZAS, []).some((x) => x.tipo === 'actual')).toBe(false);
  });

  it('el capítulo donde estoy se mueve solo y se suma a los fijos', () => {
    const p = { ...aPerfil({ todos: false, actual: true, lista: ['Secciones/C'] }) };
    expect(p.parts).toBe('current');
    expect(capitulosQueEntran(p, PARTES, 'Secciones/A')).toEqual(['Secciones/A', 'Secciones/C']);
    expect(capitulosQueEntran(p, PARTES, 'Secciones/B')).toEqual(['Secciones/B', 'Secciones/C']);
    expect(seleccionDe(p)).toEqual({ todos: false, actual: true, lista: ['Secciones/C'] });
    expect(aPerfil({ todos: true, actual: true, lista: [] }).parts).toBe('all');
    expect(aPerfil({ todos: false, actual: false, lista: ['Secciones/A'] }).parts).toBe('list');
  });

  it('lo que sale respeta las piezas que el perfil saca', () => {
    const p = { parts: 'current' as const, omit: ['toc', 'pdf:Contra.pdf'] };
    expect(loQueSale(p, PIEZAS, PARTES, 'Secciones/B').map((x) => x.nombre)).toEqual(['Tapa', 'B', 'Bibliografía']);
  });

  it('el nombre de salida usa {n}, {cap}, {nombre} y {perfil} como el core', () => {
    expect(nombreDeSalida('{n:02} - {nombre}.pdf', 3, '8', 'Butterworth', 'x')).toBe('03 - Butterworth.pdf');
    expect(nombreDeSalida('{cap:02} - {nombre}', 1, '9', 'Cauer', 'x')).toBe('09 - Cauer.pdf');
    expect(nombreDeSalida('{perfil}', 1, '', 'Libro', 'entrega')).toBe('entrega.pdf');
    expect(nombreDeSalida('{nombre}.pdf', 1, '', 'a/b: c', 'x')).toBe('a-b- c.pdf');
  });

  it('la carpeta elegida queda relativa a la raíz', () => {
    expect(carpetaRelativa('C:\\Libro\\Proyecto', 'C:\\Libro\\PDF')).toBe('../PDF');
    expect(carpetaRelativa('C:\\Libro\\Proyecto', 'c:\\libro\\proyecto\\salida\\pdf')).toBe('salida/pdf');
    expect(carpetaRelativa('C:\\Libro\\Proyecto', 'D:\\Otro')).toBeNull();
    expect(carpetaRelativa('C:\\Libro\\Proyecto', 'C:\\Libro\\Proyecto')).toBe('.');
  });
});
