import { describe, expect, it } from 'vitest';
import type { DisponibilidadTexpresso, VistaTexpresso } from '../backend';
import { boton, esFuenteTex } from './texpresso';

const vista = (o: Partial<VistaTexpresso> = {}): VistaTexpresso => ({ fase: 'apagado', motivo: null, distro: 'Ubuntu', reinicios: 0, arranqueMs: null, ultimoMs: null, errores: 0, ...o });
const disp = (o: Partial<DisponibilidadTexpresso> = {}): DisponibilidadTexpresso => ({ disponible: true, motivo: null, distro: 'Ubuntu', texpresso: null, completo: false, ...o });

describe('el botón de TeXpresso', () => {
  it('apagado y disponible: se puede prender', () => {
    const b = boton(vista(), disp(), false);
    expect(b).toMatchObject({ texto: '⚡ TeXpresso', clase: 'apagado', deshabilitado: false, prender: true });
  });

  it('sin WSL, sin la distro o sin TeXpresso: deshabilitado, con el motivo en el título', () => {
    const b = boton(vista(), disp({ disponible: false, motivo: 'falta TeX Live en Ubuntu' }), false);
    expect(b.deshabilitado).toBe(true);
    expect(b.texto).toContain('no disponible');
    expect(b.titulo).toContain('falta TeX Live en Ubuntu');
    // Lo mismo si el sondeo al prender dijo que no.
    expect(boton(vista({ fase: 'no_disponible', motivo: 'TeXpresso no está instalado' }), disp(), false).titulo).toContain('no está instalado');
  });

  it('mientras se mira la distro, iniciando, listo y caído', () => {
    expect(boton(vista(), disp(), true)).toMatchObject({ deshabilitado: true, clase: 'iniciando' });
    expect(boton(vista({ fase: 'iniciando' }), disp(), false)).toMatchObject({ clase: 'iniciando', prender: false, deshabilitado: false });
    expect(boton(vista({ fase: 'listo', ultimoMs: 15 }), disp(), false).texto).toBe('⚡ TeXpresso: listo · 15 ms');
    const conError = boton(vista({ fase: 'listo', errores: 2, motivo: 'Undefined control sequence.' }), disp(), false);
    expect(conError.texto).toBe('⚡ TeXpresso: listo · 2 errores');
    expect(conError.clase).toBe('listo con-errores');
    expect(conError.titulo).toContain('Undefined control sequence.');
    expect(boton(vista({ fase: 'caido', motivo: 'TeXpresso no respondió en 6 s; reiniciando' }), disp(), false).texto).toContain('reiniciando');
    const caido = boton(vista({ fase: 'caido', motivo: 'se reinició 3 veces' }), disp(), false);
    expect(caido).toMatchObject({ texto: '⚡ TeXpresso: caído', prender: false, deshabilitado: false });
  });

  it('sin estado todavía (proyecto recién abierto) queda deshabilitado', () => {
    expect(boton(null, null, false).deshabilitado).toBe(true);
  });

  it('sólo se le mandan los archivos de texto de TeX', () => {
    expect(esFuenteTex('Secciones/Capítulo.tex')).toBe(true);
    expect(esFuenteTex('estilo.STY')).toBe(true);
    expect(esFuenteTex('bibliography.bib')).toBe(true);
    expect(esFuenteTex('Imagenes/tapa.pdf')).toBe(false);
    expect(esFuenteTex('foto.png')).toBe(false);
  });
});
