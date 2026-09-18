'use strict';

function unescape(s) {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

function escape(s) {
  return encodeURIComponent(s);
}

function parse(str, sep = '&', eq = '=', options) {
  const out = Object.create(null);
  if (typeof str !== 'string' || str.length === 0) return out;
  const max = options && options.maxKeys !== undefined ? options.maxKeys : 1000;
  let parts = str.split(sep);
  if (max > 0) parts = parts.slice(0, max);
  for (const p of parts) {
    if (!p) continue;
    const i = p.indexOf(eq);
    const k = unescape((i >= 0 ? p.slice(0, i) : p).replace(/\+/g, ' '));
    const v = unescape((i >= 0 ? p.slice(i + eq.length) : '').replace(/\+/g, ' '));
    if (k in out) {
      if (Array.isArray(out[k])) out[k].push(v);
      else out[k] = [out[k], v];
    } else out[k] = v;
  }
  return out;
}

function stringifyPrimitive(v) {
  if (typeof v === 'string') return v;
  if (typeof v === 'number' && isFinite(v)) return '' + v;
  if (typeof v === 'bigint') return '' + v;
  if (typeof v === 'boolean') return v ? 'true' : 'false';
  return '';
}

function stringify(obj, sep = '&', eq = '=') {
  if (obj === null || typeof obj !== 'object') return '';
  const parts = [];
  for (const k of Object.keys(obj)) {
    const v = obj[k];
    const ek = escape(stringifyPrimitive(k));
    if (Array.isArray(v)) {
      for (const x of v) parts.push(ek + eq + escape(stringifyPrimitive(x)));
    } else {
      parts.push(ek + eq + escape(stringifyPrimitive(v)));
    }
  }
  return parts.join(sep);
}

module.exports = { parse, stringify, escape, unescape, decode: parse, encode: stringify };
