import { describe, expect, it } from 'vitest';
import { aHtml, bloques, fragmentoEnCursor } from './aproximada';

const f = (tex: string, bloque: boolean) => `<${bloque ? 'B' : 'M'}>${tex}</${bloque ? 'B' : 'M'}>`;

describe('vista aproximada', () => {
  it('traduce el texto común y deja las fórmulas al renderizador', () => {
    expect(aHtml('Esta es \\textbf{Butterworth} y $G(\\omega)$.', f)).toBe('Esta es <b>Butterworth</b> y <M>G(\\omega)</M>.');
    expect(aHtml('en 1930~\\parencite{butter1930}', f)).toBe('en 1930\u00a0<span class="aprox-cita">[butter1930]</span>');
    expect(aHtml('ver~\\eqref{eq:x} --- y 1--2', f)).toBe('ver\u00a0<span class="aprox-ref">(eq:x)</span> — y 1–2');
    expect(aHtml('a % comentario\nb', f)).toBe('a \nb');
    expect(aHtml('\\label{x}\\index{y}texto', f)).toBe('texto');
  });

  it('los entornos de fórmula van enteros a KaTeX, sin \\label', () => {
    expect(aHtml('\\begin{align}a &= b \\label{e}\\\\ c&=d\\end{align}', f)).toBe('<B>\\begin{aligned}a &= b \\\\ c&=d\\end{aligned}</B>');
    expect(aHtml('\\[x^2\\]', f)).toBe('<B>x^2</B>');
  });

  it('las cajas del libro son un div con su nombre y TikZ queda como recuadro', () => {
    expect(aHtml('\\begin{destacado}Hola\\end{destacado}', f)).toBe('<div class="aprox-env aprox-env-destacado" data-env="destacado">Hola</div>');
    expect(aHtml('\\begin{tikzpicture}\\draw (0,0);\\end{tikzpicture}', f)).toBe('<div class="aprox-opaco">figura TikZ</div>');
    expect(aHtml('\\begin{itemize}\\item a\\item b\\end{itemize}', f)).toBe('<ul><li> a<li> b</ul>');
  });

  it('el archivo entero se corta en párrafos sin partir un entorno y sin el preámbulo', () => {
    const t = 'uno\n\ndos\n\n\\begin{destacado}\na\n\nb\n\\end{destacado}\n\n% \\begin{x}\ntres\n\n\ncuatro';
    expect(bloques(t).map((b) => b.texto)).toEqual(['uno', 'dos', '\\begin{destacado}\na\n\nb\n\\end{destacado}', '% \\begin{x}\ntres', 'cuatro']);
    const conDoc = '\\documentclass{book}\n\\usepackage{x}\n\\begin{document}\nhola\n\nchau\n\\end{document}\n';
    expect(bloques(conDoc).map((b) => b.texto.trim())).toEqual(['hola', 'chau']);
    const b = bloques(t)[1];
    expect(t.slice(b.desde, b.hasta)).toBe('dos');
  });

  it('el fragmento del cursor es el párrafo o la fórmula que lo contiene', () => {
    const t = 'uno\n\ndos tres\n\n\\begin{equation}\nx=1\n\\end{equation}\n\ncuatro';
    expect(fragmentoEnCursor(t, 7).texto).toBe('dos tres');
    expect(fragmentoEnCursor(t, t.indexOf('x=1')).texto).toBe('\\begin{equation}\nx=1\n\\end{equation}');
    expect(fragmentoEnCursor(t, t.length).texto).toBe('cuatro');
  });
});
