// Vista aproximada: el párrafo (o la fórmula) donde está el cursor, traducido a HTML en milisegundos y sin TeX. Las fórmulas van con
// KaTeX; el texto, con lo común (negrita, cursiva, títulos, listas, citas, cajas). No es la página real (no corta líneas ni páginas
// como TeX, no conoce las macros del proyecto ni dibuja TikZ): sirve para ver mientras se escribe si una fórmula quedó bien, y la vista
// previa de verdad llega después. Funciones puras: el renderizador de fórmulas se recibe por parámetro (los tests no cargan KaTeX).

export type Formula = (tex: string, enBloque: boolean) => string;

/** Entornos matemáticos de bloque, y cómo se le pasan a KaTeX (que no conoce `align`, pero sí `aligned`). */
const MATE: Record<string, (c: string) => string> = {
  equation: (c) => c,
  'equation*': (c) => c,
  displaymath: (c) => c,
  align: (c) => `\\begin{aligned}${c}\\end{aligned}`,
  'align*': (c) => `\\begin{aligned}${c}\\end{aligned}`,
  gather: (c) => `\\begin{gathered}${c}\\end{gathered}`,
  'gather*': (c) => `\\begin{gathered}${c}\\end{gathered}`,
  multline: (c) => `\\begin{gathered}${c}\\end{gathered}`,
  'multline*': (c) => `\\begin{gathered}${c}\\end{gathered}`,
  eqnarray: (c) => `\\begin{aligned}${c.replace(/&([^&]*)&/g, '&$1')}\\end{aligned}`,
};
/** Lo que no se puede dibujar sin TeX: queda un recuadro con su nombre. */
const OPACOS = new Set(['tikzpicture', 'pgfpicture', 'tabular', 'tabularx', 'longtable']);

const escapar = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** El párrafo o el entorno matemático que contiene a `pos`: un entorno de fórmula entero si el cursor está adentro; si no, las
 *  líneas entre dos líneas en blanco (en el libro cada párrafo es una sola línea). */
export function fragmentoEnCursor(texto: string, pos: number): { desde: number; hasta: number; texto: string } {
  const re = /\\begin\{([a-zA-Z*]+)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(texto)) && m.index <= pos) {
    const env = m[1];
    if (!(env in MATE)) continue;
    const fin = texto.indexOf(`\\end{${env}}`, m.index);
    if (fin >= 0 && pos <= fin + env.length + 6) {
      const hasta = fin + env.length + 6;
      return { desde: m.index, hasta, texto: texto.slice(m.index, hasta) };
    }
  }
  const antes = texto.lastIndexOf('\n\n', Math.max(0, pos - 1));
  const desde = antes < 0 ? 0 : antes + 2;
  const despues = texto.indexOf('\n\n', pos);
  const hasta = despues < 0 ? texto.length : despues;
  return { desde, hasta, texto: texto.slice(desde, hasta) };
}

export interface Bloque {
  desde: number;
  hasta: number;
  texto: string;
}

/** El archivo en bloques para dibujarlo entero: párrafos separados por líneas en blanco, sin partir un entorno (una caja `destacado`
 *  o una figura con líneas en blanco adentro queda en un solo bloque). Si el archivo tiene `\begin{document}`, sólo lo de adentro: el
 *  preámbulo no se ve. */
export function bloques(texto: string): Bloque[] {
  let ini = 0;
  let fin = texto.length;
  const bd = texto.indexOf('\\begin{document}');
  if (bd >= 0) {
    ini = bd + '\\begin{document}'.length;
    const ed = texto.indexOf('\\end{document}', ini);
    if (ed >= 0) fin = ed;
  }
  const out: Bloque[] = [];
  const re = /\r?\n(?:[ \t]*\r?\n)+/g; // también CRLF (el editor normaliza a \n, un archivo leído del disco no)
  re.lastIndex = ini;
  let desde = ini;
  let contado = ini; // hasta dónde se contaron los \begin y \end del bloque en curso
  let abiertos = 0;
  const contar = (s: string) => (s.match(/\\begin\{/g)?.length ?? 0) - (s.match(/\\end\{/g)?.length ?? 0);
  let m: RegExpExecArray | null;
  while ((m = re.exec(texto)) && m.index < fin) {
    abiertos += contar(sinComentarios(texto.slice(contado, m.index)));
    contado = m.index;
    if (abiertos > 0) continue; // un entorno abierto: el bloque sigue
    const t = texto.slice(desde, m.index);
    if (t.trim()) out.push({ desde, hasta: m.index, texto: t });
    desde = contado = m.index + m[0].length;
    abiertos = 0;
  }
  const resto = texto.slice(desde, fin);
  if (resto.trim()) out.push({ desde, hasta: fin, texto: resto });
  return out;
}

function sinComentarios(t: string): string {
  return t
    .split('\n')
    .map((l) => {
      for (let i = 0; i < l.length; i++) if (l[i] === '%' && (i === 0 || l[i - 1] !== '\\')) return l.slice(0, i);
      return l;
    })
    .join('\n');
}

/** El argumento entre llaves que empieza en `i` (después de espacios), o null. Devuelve el contenido y dónde termina. */
function argumento(t: string, i: number): { arg: string; fin: number } | null {
  let j = i;
  while (t[j] === ' ') j++;
  if (t[j] === '[') {
    const k = t.indexOf(']', j);
    if (k < 0) return null;
    j = k + 1;
  }
  if (t[j] !== '{') return null;
  let prof = 0;
  for (let k = j; k < t.length; k++) {
    if (t[k] === '\\') {
      k++;
      continue;
    }
    if (t[k] === '{') prof++;
    else if (t[k] === '}' && --prof === 0) return { arg: t.slice(j + 1, k), fin: k + 1 };
  }
  return null;
}

const ENVOLTORIOS: Record<string, [string, string]> = {
  textbf: ['<b>', '</b>'],
  bfseries: ['<b>', '</b>'],
  textit: ['<i>', '</i>'],
  emph: ['<em>', '</em>'],
  textsl: ['<i>', '</i>'],
  texttt: ['<code>', '</code>'],
  underline: ['<u>', '</u>'],
  textsc: ['<span style="font-variant:small-caps">', '</span>'],
  chapter: ['<h2>', '</h2>'],
  'chapter*': ['<h2>', '</h2>'],
  section: ['<h3>', '</h3>'],
  'section*': ['<h3>', '</h3>'],
  subsection: ['<h4>', '</h4>'],
  'subsection*': ['<h4>', '</h4>'],
  subsubsection: ['<h5>', '</h5>'],
  paragraph: ['<b>', '</b> '],
  footnote: ['<sup class="aprox-nota">(', ')</sup>'],
  caption: ['<div class="aprox-caption">', '</div>'],
};
const SE_VAN = new Set(['label', 'index', 'FigurasDe', 'vspace', 'hspace', 'addcontentsline', 'markboth', 'markright', 'phantomsection', 'centering', 'noindent', 'clearpage', 'cleardoublepage', 'newpage', 'medskip', 'bigskip', 'smallskip']);
const CITAS = new Set(['cite', 'parencite', 'textcite', 'autocite', 'footcite', 'citep', 'citet']);
const REFS = new Set(['ref', 'eqref', 'pageref', 'autoref', 'cref', 'Cref']);

/** El texto de LaTeX a HTML. */
export function aHtml(tex: string, formula: Formula): string {
  const t = sinComentarios(tex);
  let out = '';
  let i = 0;
  const pila: string[] = [];
  const texto = (s: string) => {
    out += escapar(s)
      .replace(/---/g, '—')
      .replace(/--/g, '–')
      .replace(/``/g, '“')
      .replace(/''/g, '”')
      .replace(/~/g, '\u00a0');
  };
  while (i < t.length) {
    const c = t[i];
    // Fórmulas en línea y de bloque.
    const delims: [string, string, boolean][] = [
      ['$$', '$$', true],
      ['\\[', '\\]', true],
      ['$', '$', false],
      ['\\(', '\\)', false],
    ];
    const d = delims.find(([a]) => t.startsWith(a, i));
    if (d) {
      const fin = t.indexOf(d[1], i + d[0].length);
      if (fin >= 0) {
        out += formula(t.slice(i + d[0].length, fin), d[2]);
        i = fin + d[1].length;
        continue;
      }
    }
    if (c === '\\') {
      const m = /^\\([a-zA-Z]+\*?|.)/.exec(t.slice(i));
      const nombre = m ? m[1] : '';
      i += 1 + nombre.length;
      if (nombre === 'begin' || nombre === 'end') {
        const a = argumento(t, i);
        if (!a) continue;
        i = a.fin;
        const env = a.arg;
        if (nombre === 'begin' && env in MATE) {
          const fin = t.indexOf(`\\end{${env}}`, i);
          const cuerpo = fin >= 0 ? t.slice(i, fin) : t.slice(i);
          out += formula(MATE[env](cuerpo.replace(/\\label\{[^}]*\}/g, '').replace(/\\nonumber|\\notag/g, '')), true);
          i = fin >= 0 ? fin + env.length + 6 : t.length;
          continue;
        }
        if (nombre === 'begin' && OPACOS.has(env)) {
          const fin = t.indexOf(`\\end{${env}}`, i);
          out += `<div class="aprox-opaco">${escapar(env === 'tikzpicture' ? 'figura TikZ' : env)}</div>`;
          i = fin >= 0 ? fin + env.length + 6 : t.length;
          continue;
        }
        if (nombre === 'begin') {
          const lista = env === 'itemize' || env === 'enumerate';
          out += lista ? (env === 'itemize' ? '<ul>' : '<ol>') : `<div class="aprox-env aprox-env-${escapar(env.replace(/\*/g, ''))}" data-env="${escapar(env)}">`;
          pila.push(env);
        } else {
          const env2 = pila.pop() ?? env;
          out += env2 === 'itemize' ? '</ul>' : env2 === 'enumerate' ? '</ol>' : '</div>';
        }
        continue;
      }
      if (nombre === 'item') {
        out += '<li>';
        continue;
      }
      if (nombre === '\\') {
        out += '<br>';
        continue;
      }
      if (nombre.length === 1 && /[%&#_{}$]/.test(nombre)) {
        out += escapar(nombre);
        continue;
      }
      if (CITAS.has(nombre) || REFS.has(nombre)) {
        const a = argumento(t, i);
        if (a) {
          i = a.fin;
          out += CITAS.has(nombre) ? `<span class="aprox-cita">[${escapar(a.arg)}]</span>` : `<span class="aprox-ref">(${escapar(a.arg)})</span>`;
        }
        continue;
      }
      if (SE_VAN.has(nombre)) {
        const a = argumento(t, i);
        if (a && !['centering', 'noindent', 'phantomsection', 'clearpage', 'cleardoublepage', 'newpage', 'medskip', 'bigskip', 'smallskip'].includes(nombre)) i = a.fin;
        continue;
      }
      const a = argumento(t, i);
      if (a) {
        i = a.fin;
        const [abre, cierra] = ENVOLTORIOS[nombre] ?? ['', ''];
        out += abre + aHtml(a.arg, formula) + cierra;
      }
      // Un comando sin argumento que no se conoce (\quad, \today...): se va.
      continue;
    }
    if (c === '{' || c === '}') {
      i++;
      continue;
    }
    if (c === '\n' && t[i + 1] === '\n') {
      out += '<p></p>';
      i += 2;
      continue;
    }
    // Hasta el próximo carácter especial.
    let j = i + 1;
    while (j < t.length && !'\\${}\n'.includes(t[j])) j++;
    texto(t.slice(i, j));
    i = j;
  }
  while (pila.length) {
    const env = pila.pop();
    out += env === 'itemize' ? '</ul>' : env === 'enumerate' ? '</ol>' : '</div>';
  }
  return out;
}
