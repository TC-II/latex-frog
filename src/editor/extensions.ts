// Extensiones de CodeMirror para un documento LaTeX. Todo lo que cambia el aspecto usa variables CSS, así el tema
// claro y el oscuro de la interfaz alcanzan al editor sin tocar nada acá.
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { bracketMatching, HighlightStyle, indentOnInput, StreamLanguage, syntaxHighlighting } from '@codemirror/language';
import { stex } from '@codemirror/legacy-modes/mode/stex';
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search';
import { Annotation, Compartment, EditorState, type Extension, type StateEffect } from '@codemirror/state';
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { extensionAutocompletar, type FuenteDeSimbolos } from './autocompletar';
import { extensionProblemas } from './problemas';

/**
 * La gramática de LaTeX. En F1 es el modo `stex` de `@codemirror/legacy-modes`; la definitiva (que sepa si el cursor
 * está en modo matemático y dentro de qué entorno) se evalúa en F5 y se cambia acá, en un solo lugar.
 */
export function latexLanguage(): Extension {
  return StreamLanguage.define(stex);
}

const resaltado = HighlightStyle.define([
  { tag: t.keyword, color: 'var(--syn-comando)' },
  { tag: t.comment, color: 'var(--syn-comentario)', fontStyle: 'italic' },
  { tag: [t.tagName, t.atom], color: 'var(--syn-entorno)' },
  { tag: [t.bracket, t.operator], color: 'var(--syn-signo)' },
  { tag: [t.number, t.string], color: 'var(--syn-valor)' },
  { tag: [t.variableName, t.meta], color: 'var(--syn-variable)' },
]);

const tema = EditorView.theme({
  '&': { height: '100%', color: 'var(--ed-texto)', backgroundColor: 'var(--ed-fondo)' },
  '.cm-scroller': { fontFamily: 'var(--fuente-mono)', fontSize: '13.5px', lineHeight: '1.55', overflow: 'auto' },
  '.cm-content': { caretColor: 'var(--ed-cursor)', padding: '6px 0' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--ed-cursor)' },
  // La selección se dibuja en una capa DETRÁS del texto (`drawSelection`). Tiene que ganarle al color de CodeMirror: el suyo con foco
  // (`&light.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground`, #d7d4f0) es más específico que cualquier regla
  // de este tema y, como el tema oscuro de la app sale de `data-theme` y CodeMirror cree que siempre es claro, sobre el
  // fondo oscuro quedaba un bloque casi blanco con el texto claro encima: ilegible. Por eso el `!important`.
  '.cm-selectionBackground': { backgroundColor: 'var(--ed-seleccion-inactiva) !important' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': { backgroundColor: 'var(--ed-seleccion) !important' },
  '.cm-content ::selection': { backgroundColor: 'var(--ed-seleccion)' },
  '.cm-gutters': { backgroundColor: 'var(--ed-gutter)', color: 'var(--ed-gutter-texto)', border: 'none' },
  // La línea activa va SOBRE la capa de la selección: si fuera opaca la tapaba (cada párrafo del libro es una sola línea, así que
  // seleccionar un párrafo no se veía). Con transparencia la selección se ve a través.
  '.cm-activeLine': { backgroundColor: 'var(--ed-linea-activa)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--ed-linea-activa)' },
  '.cm-matchingBracket': { backgroundColor: 'var(--ed-par)', outline: '1px solid var(--ed-par-borde)' },
  '.cm-panels': { backgroundColor: 'var(--panel)', color: 'var(--texto)' },
  '.cm-searchMatch': { backgroundColor: 'var(--ed-busqueda)' },
  // El panel de buscar y reemplazar (Ctrl+F / Ctrl+H). CodeMirror le pone a los campos y botones un 70 % y a las etiquetas un 80 %
  // del tamaño del editor, que acá es el del body (13 px): quedaban en ~9 px, ilegibles. Se llevan al tamaño del resto de la
  // interfaz y a sus colores (el degradé de sus botones es del tema claro y en el oscuro desentonaba). Las reglas van con `&` para
  // empatar en especificidad con las `&light .cm-button` de CodeMirror y ganarles por orden.
  '& .cm-panel.cm-search': { padding: '6px 32px 6px 10px', fontFamily: 'var(--fuente)', fontSize: '13px', lineHeight: '1.4' },
  '& .cm-panel.cm-search label': { fontSize: '13px', color: 'var(--texto-suave)', marginRight: '10px', cursor: 'pointer' },
  '& .cm-panel.cm-search input[type=checkbox]': { margin: '0 4px 0 0', verticalAlign: 'middle' },
  '& .cm-panel.cm-search .cm-textfield': {
    fontSize: '13px', fontFamily: 'var(--fuente-mono)', padding: '3px 8px', margin: '2px 6px 2px 0', minWidth: '240px',
    border: '1px solid var(--borde)', borderRadius: '4px', backgroundColor: 'var(--ed-fondo)', color: 'var(--texto)', outline: 'none',
  },
  '& .cm-panel.cm-search .cm-textfield:focus': { borderColor: 'var(--acento)', boxShadow: '0 0 0 1px var(--acento)' },
  '& .cm-panel.cm-search .cm-button': {
    fontSize: '13px', fontFamily: 'var(--fuente)', padding: '3px 10px', margin: '2px 4px 2px 0', cursor: 'pointer',
    border: '1px solid var(--borde)', borderRadius: '4px', backgroundImage: 'none', backgroundColor: 'var(--panel)', color: 'var(--texto)',
  },
  '& .cm-panel.cm-search .cm-button:hover': { backgroundColor: 'var(--hover)' },
  '& .cm-panel.cm-search [name=close]': { fontSize: '20px', lineHeight: '1', top: '6px', right: '10px', padding: '0 4px', color: 'var(--texto-suave)', cursor: 'pointer' },
});

/** Los textos del panel de búsqueda (y de los demás de CodeMirror) en español, como el resto de la interfaz. */
const frases = EditorState.phrases.of({
  Find: 'Buscar',
  Replace: 'Reemplazar',
  next: 'siguiente',
  previous: 'anterior',
  all: 'todas',
  'match case': 'mayúsculas',
  regexp: 'regex',
  'by word': 'palabra completa',
  replace: 'reemplazar',
  'replace all': 'reemplazar todas',
  close: 'cerrar',
  'current match': 'coincidencia actual',
  'on line': 'en la línea',
  'replaced match on line $': 'reemplazada la coincidencia de la línea $',
  'replaced $ matches': '$ coincidencias reemplazadas',
  'Go to line': 'Ir a la línea',
  go: 'ir',
});

// El editor no importa el store (el store importa el editor): lo que necesita de él entra por estos ganchos.
let fuenteSimbolos: FuenteDeSimbolos = () => null;
let alClicDefinicion: () => void = () => {};

/** De dónde saca el autocompletado el índice del proyecto. */
export function setFuenteSimbolos(f: FuenteDeSimbolos): void {
  fuenteSimbolos = f;
}

/** Qué hace Ctrl+clic (ir a la definición de lo que hay bajo el cursor). */
export function setManejadorDefinicion(f: () => void): void {
  alClicDefinicion = f;
}

let alDobleClic: () => void = () => {};

/** Qué hace el doble clic en el código (saltar al PDF y resaltar la palabra). */
export function setManejadorDobleClic(f: () => void): void {
  alDobleClic = f;
}

/** Ctrl+clic: CodeMirror lo usa para agregar un cursor; acá es "ir a la definición" (el multicursor sigue con Alt+clic). */
const ctrlClic = EditorView.domEventHandlers({
  // Doble clic: CodeMirror ya seleccionó la palabra; con eso se salta al PDF y se resalta esa palabra exacta (como el doble clic en el
  // PDF lleva a la palabra del código). Va después del evento, cuando la selección ya está puesta.
  dblclick() {
    setTimeout(() => alDobleClic(), 0);
    return false;
  },
  mousedown(e, view) {
    if (e.button !== 0 || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return false;
    const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos === null) return false;
    e.preventDefault();
    view.dispatch({ selection: { anchor: pos } });
    alClicDefinicion();
    return true;
  },
});

// Resaltar las otras apariciones de lo seleccionado sirve cuando el usuario selecciona algo, pero no cuando la selección la pone un
// salto (PDF → código selecciona la palabra clickeada): ahí se encendían todas las palabras iguales del archivo y la que importaba no
// se distinguía. El salto apaga el resaltado (`efectosDeSalto`) y se vuelve a prender con la primera selección que haga el usuario.
const coincidencias = new Compartment();
const SIN_COINCIDENCIAS: Extension = [];
const deSalto = Annotation.define<boolean>();

/** Lo que tiene que llevar el `dispatch` de un salto que selecciona una palabra para que no se resalten sus repeticiones. */
export function efectosDeSalto(): { effects: StateEffect<unknown>; annotations: Annotation<boolean> } {
  return { effects: coincidencias.reconfigure(SIN_COINCIDENCIAS), annotations: deSalto.of(true) };
}

/** Si el estado resalta las repeticiones de lo seleccionado (para los tests). */
export function resaltaCoincidencias(state: EditorState): boolean {
  return coincidencias.get(state) !== SIN_COINCIDENCIAS;
}

const reactivarCoincidencias = EditorState.transactionExtender.of((tr) =>
  tr.selection && !tr.annotation(deSalto) && coincidencias.get(tr.startState) === SIN_COINCIDENCIAS
    ? { effects: coincidencias.reconfigure(highlightSelectionMatches()) }
    : null,
);

/** Soft wrap por defecto: cada párrafo del libro es una sola línea de hasta ~500 caracteres. */
export function extensionesBase(): Extension[] {
  return [
    lineNumbers(),
    highlightActiveLineGutter(),
    history(),
    drawSelection(),
    EditorState.allowMultipleSelections.of(true),
    EditorState.tabSize.of(2),
    indentOnInput(),
    bracketMatching(),
    highlightActiveLine(),
    coincidencias.of(highlightSelectionMatches()),
    reactivarCoincidencias,
    search({ top: true }),
    EditorView.lineWrapping,
    latexLanguage(),
    syntaxHighlighting(resaltado),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    extensionProblemas(),
    extensionAutocompletar(() => fuenteSimbolos()),
    ctrlClic,
    tema,
    frases,
  ];
}
