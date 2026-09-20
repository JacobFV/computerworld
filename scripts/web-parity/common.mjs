// Shared by dump.mjs and compare.mjs: which computed properties both sides report and
// how each one is compared.

/// The computed properties both sides report, as strings, in this order.
export const PROPERTIES = [
  'display', 'position', 'float', 'width', 'height',
  'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width',
  'font-family', 'font-size', 'font-weight', 'line-height',
  'color', 'background-color', 'text-align', 'white-space', 'vertical-align',
  'overflow-x', 'overflow-y', 'z-index', 'box-sizing',
];

/// Properties whose values are lengths (compared numerically, with tolerance) rather
/// than keywords (compared as strings).
export const LENGTH_PROPERTIES = new Set([
  'width', 'height',
  'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width',
  'font-size', 'line-height',
]);

/// `font-family` is reported, not compared: the two engines have different font stacks.
export const INFORMATIONAL = new Set(['font-family']);

/// Tolerances in CSS px. Positions and sizes must agree within `RECT_PX`; widths and
/// heights of boxes whose size follows the text (inline boxes, shrink-to-fit blocks such
/// as floats, tables and inline-blocks, and table cells) get `TEXT_PX`, because the two
/// engines shape text with different faces and advances are quantised differently.
export const RECT_PX = 1;
export const TEXT_PX = 2;

/// Whether an element's width and height depend on the text it contains.
export function textDependent(node) {
  const c = node.computed ?? {};
  const d = c.display ?? '';
  if (c.float && c.float !== 'none') return true;
  if (c.position === 'absolute' || c.position === 'fixed') return true;
  return d.startsWith('inline') || d.startsWith('table') || d === 'list-item' && c.width === 'auto';
}

/// Chromium spells a few values its own way; both sides are folded to the same spelling.
export function normalise(property, value) {
  let v = String(value ?? '').trim();
  if (property === 'text-align') v = v.replace(/^-webkit-/, '');
  if (property === 'font-weight') v = v === 'normal' ? '400' : v === 'bold' ? '700' : v;
  if (property === 'color' || property === 'background-color') v = normaliseColour(v);
  return v;
}

/// `rgb(r, g, b)` / `rgba(r, g, b, a)` with the alpha rounded so `0.502` and `0.5` agree.
export function normaliseColour(v) {
  const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(v);
  if (!m) return v;
  const a = m[4] === undefined ? 1 : Number(m[4]);
  if (a >= 1) return `rgb(${m[1]}, ${m[2]}, ${m[3]})`;
  return `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${Math.round(a * 100) / 100})`;
}

/// A length string (`12px`, `auto`, `normal`) as a number of px, or null when it is not one.
export function px(v) {
  const m = /^(-?[\d.]+)px$/.exec(String(v ?? '').trim());
  return m ? Number(m[1]) : null;
}
