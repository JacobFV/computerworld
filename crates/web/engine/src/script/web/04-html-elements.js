// Prelude part 4: the HTML element classes with their IDL attributes, and the
// canvas 2D context.
(function () {
'use strict';
const C = globalThis['%core'];
const { W, hooks, define, domError, typeError, HTMLElement, reflectString, reflectBool, reflectInt, reflectUrl, fire, fireSimple, queueTask } = C;

const classes = {};
const tags = {};
function element(name, tagList, body) {
  const cls = body(HTMLElement);
  Object.defineProperty(cls, 'name', { value: name });
  classes[name] = cls;
  for (const t of tagList) tags[t] = cls;
  return cls;
}

// ---------------------------------------------------------------- URL parts

function urlParts(href) {
  try { return new URL(href); } catch (e) { return null; }
}
function defineUrlParts(proto, attr) {
  const parts = ['protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash', 'username', 'password'];
  for (const p of parts) {
    Object.defineProperty(proto, p, {
      get() { const u = urlParts(this[attr]); return u ? u[p] : ''; },
      set(v) { const u = urlParts(this[attr]); if (u) { u[p] = v; this.setAttribute(attr, u.href); } },
      configurable: true, enumerable: true,
    });
  }
  Object.defineProperty(proto, 'origin', { get() { const u = urlParts(this[attr]); return u ? u.origin : ''; }, configurable: true, enumerable: true });
}

// ---------------------------------------------------------------- simple elements

element('HTMLAnchorElement', ['a'], (B) => class HTMLAnchorElement extends B {
  toString() { return this.href; }
  get relList() { let c = this['%relList']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'rel'); define(this, '%relList', c); } return c; }
  get text() { return this.textContent; }
  set text(v) { this.textContent = v; }
});
reflectUrl(classes.HTMLAnchorElement.prototype, 'href');
for (const p of ['target', 'rel', 'download', 'hreflang', 'type', 'ping', 'referrerPolicy', 'name', 'charset', 'coords', 'rev', 'shape']) reflectString(classes.HTMLAnchorElement.prototype, p);
defineUrlParts(classes.HTMLAnchorElement.prototype, 'href');
element('HTMLAreaElement', ['area'], (B) => class HTMLAreaElement extends B { toString() { return this.href; } });
reflectUrl(classes.HTMLAreaElement.prototype, 'href');
for (const p of ['target', 'rel', 'alt', 'coords', 'shape']) reflectString(classes.HTMLAreaElement.prototype, p);
defineUrlParts(classes.HTMLAreaElement.prototype, 'href');

element('HTMLImageElement', ['img'], (B) => class HTMLImageElement extends B {
  get complete() { return true; }
  get naturalWidth() { const s = C.imageSize(this); return s ? s[0] : (this.hasAttribute('src') ? this.width : 0); }
  get naturalHeight() { const s = C.imageSize(this); return s ? s[1] : (this.hasAttribute('src') ? this.height : 0); }
  get width() { const w = this.getAttribute('width'); if (w !== null && !Number.isNaN(parseInt(w, 10))) return parseInt(w, 10); return W.isRendered(this) ? this.offsetWidth : (C.imageSize(this) || [0, 0])[0]; }
  set width(v) { this.setAttribute('width', String(v | 0)); }
  get height() { const h = this.getAttribute('height'); if (h !== null && !Number.isNaN(parseInt(h, 10))) return parseInt(h, 10); return W.isRendered(this) ? this.offsetHeight : (C.imageSize(this) || [0, 0])[1]; }
  set height(v) { this.setAttribute('height', String(v | 0)); }
  get currentSrc() { return this.src; }
  get x() { return this.getBoundingClientRect().x; }
  get y() { return this.getBoundingClientRect().y; }
  decode() { return Promise.resolve(); }
});
reflectUrl(classes.HTMLImageElement.prototype, 'src');
for (const p of ['alt', 'srcset', 'sizes', 'crossOrigin', 'useMap', 'referrerPolicy', 'loading', 'decoding', 'fetchPriority', 'align', 'name']) reflectString(classes.HTMLImageElement.prototype, p, p.toLowerCase());
reflectBool(classes.HTMLImageElement.prototype, 'isMap');
reflectInt(classes.HTMLImageElement.prototype, 'hspace', 'hspace', 0);
reflectInt(classes.HTMLImageElement.prototype, 'vspace', 'vspace', 0);
// `new Image(w, h)`
function Image(width, height) {
  const el = document.createElement('img');
  if (width !== undefined) el.setAttribute('width', String(width | 0));
  if (height !== undefined) el.setAttribute('height', String(height | 0));
  return el;
}
Image.prototype = classes.HTMLImageElement.prototype;
C.imageInserted = (el) => { if (el.hasAttribute('src')) queueTask(() => { if (el.isConnected || true) fireSimple(el, 'load', false, false); }); };
C.imageSize = (el) => null;

element('HTMLInputElement', ['input'], (B) => class HTMLInputElement extends B {
  get type() { const t = (this.getAttribute('type') || 'text').toLowerCase(); return ['text', 'search', 'url', 'tel', 'email', 'password', 'date', 'month', 'week', 'time', 'datetime-local', 'number', 'range', 'color', 'checkbox', 'radio', 'file', 'submit', 'image', 'reset', 'button', 'hidden'].includes(t) ? t : 'text'; }
  set type(v) { this.setAttribute('type', String(v)); }
  get value() {
    const t = this.type;
    if (t === 'file') return this.files.length ? 'C:\\fakepath\\' + this.files[0].name : '';
    if (t === 'checkbox' || t === 'radio') { const v = this.getAttribute('value'); return v === null ? 'on' : v; }
    if (t === 'number') { const v = W.value(this); return v === '' || Number.isNaN(Number(v)) ? '' : v; }
    return W.value(this);
  }
  set value(v) {
    const t = this.type;
    if (t === 'file') { if (v !== '' && v !== null) throw domError('InvalidStateError', 'This input element accepts a filename, which may only be programmatically set to the empty string.'); return; }
    if (t === 'checkbox' || t === 'radio' || t === 'hidden' || t === 'submit' || t === 'button' || t === 'reset' || t === 'image') { this.setAttribute('value', v === null ? '' : String(v)); return; }
    W.setValue(this, v === null || v === undefined ? '' : String(v));
  }
  get defaultValue() { return this.getAttribute('value') || ''; }
  set defaultValue(v) { this.setAttribute('value', String(v)); }
  get checked() { return W.checked(this); }
  set checked(v) { W.setChecked(this, !!v); }
  get defaultChecked() { return this.hasAttribute('checked'); }
  set defaultChecked(v) { if (v) this.setAttribute('checked', ''); else this.removeAttribute('checked'); }
  get indeterminate() { return W.indeterminate(this); }
  set indeterminate(v) { W.indeterminate(this, !!v); }
  get form() { return W.formOwner(this); }
  get files() { let f = this['%files']; if (!f) { f = new FileList([]); define(this, '%files', f); } return f; }
  set files(v) { if (v instanceof FileList) define(this, '%files', v); }
  get valueAsNumber() {
    const t = this.type; const v = this.value;
    if (v === '') return NaN;
    if (t === 'number' || t === 'range') return Number(v);
    if (t === 'date' || t === 'month' || t === 'week' || t === 'time' || t === 'datetime-local') { const d = this.valueAsDate; return d ? d.getTime() : NaN; }
    return NaN;
  }
  set valueAsNumber(v) { if (this.type === 'number' || this.type === 'range') this.value = Number.isNaN(+v) ? '' : String(+v); else if (this.type === 'date') this.valueAsDate = new Date(+v); }
  get valueAsDate() {
    const t = this.type; const v = this.value;
    if (t === 'date') { const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(v); return m ? new Date(Date.UTC(+m[1], +m[2] - 1, +m[3])) : null; }
    if (t === 'time') { const m = /^(\d{2}):(\d{2})(?::(\d{2}))?/.exec(v); return m ? new Date(Date.UTC(1970, 0, 1, +m[1], +m[2], +(m[3] || 0))) : null; }
    if (t === 'month') { const m = /^(\d{4})-(\d{2})$/.exec(v); return m ? new Date(Date.UTC(+m[1], +m[2] - 1, 1)) : null; }
    return null;
  }
  set valueAsDate(d) {
    if (d === null || Number.isNaN(d.getTime())) { this.value = ''; return; }
    const p = (n) => String(n).padStart(2, '0');
    if (this.type === 'date') this.value = d.getUTCFullYear() + '-' + p(d.getUTCMonth() + 1) + '-' + p(d.getUTCDate());
    else if (this.type === 'time') this.value = p(d.getUTCHours()) + ':' + p(d.getUTCMinutes());
    else if (this.type === 'month') this.value = d.getUTCFullYear() + '-' + p(d.getUTCMonth() + 1);
  }
  get selectionStart() { return this._selectable() ? W.selection(this)[0] : null; }
  set selectionStart(v) { if (this._selectable()) W.selection(this, +v || 0, Math.max(+v || 0, W.selection(this)[1])); }
  get selectionEnd() { return this._selectable() ? W.selection(this)[1] : null; }
  set selectionEnd(v) { if (this._selectable()) W.selection(this, Math.min(W.selection(this)[0], +v || 0), +v || 0); }
  get selectionDirection() { return this._selectable() ? 'none' : null; }
  set selectionDirection(v) {}
  _selectable() { return ['text', 'search', 'url', 'tel', 'password'].includes(this.type); }
  select() { if (this._selectable() || this.type === 'number') W.selection(this, 0, this.value.length); }
  setSelectionRange(s, e) { if (!this._selectable()) throw domError('InvalidStateError', "The input element's type ('" + this.type + "') does not support selection."); W.selection(this, +s || 0, +e || 0); }
  setRangeText(text, start, end) {
    const v = this.value;
    if (start === undefined) { const sel = W.selection(this); start = sel[0]; end = sel[1]; }
    this.value = v.slice(0, start) + text + v.slice(end);
    W.selection(this, start, start + String(text).length);
  }
  stepUp(n) { this._step(n === undefined ? 1 : +n); }
  stepDown(n) { this._step(-(n === undefined ? 1 : +n)); }
  _step(n) { if (this.type !== 'number' && this.type !== 'range') throw domError('InvalidStateError', 'This form element is not steppable.'); const step = parseFloat(this.getAttribute('step')) || 1; const cur = this.value === '' ? 0 : Number(this.value); let v = cur + n * step; const min = parseFloat(this.getAttribute('min')); const max = parseFloat(this.getAttribute('max')); if (!Number.isNaN(min)) v = Math.max(min, v); if (!Number.isNaN(max)) v = Math.min(max, v); this.value = String(v); }
  get labels() { return this.type === 'hidden' ? null : W.collection(this, 'labels', '', 'NodeList'); }
  get list() { const id = this.getAttribute('list'); if (!id) return null; const el = document.getElementById(id); return el && el.localName === 'datalist' ? el : null; }
  get willValidate() { return !W.isDisabled(this) && !this.readOnly && !['hidden', 'reset', 'button'].includes(this.type) && !this.closest('datalist'); }
  get validity() { return C.validityOf(this); }
  get validationMessage() { return C.validationMessageOf(this); }
  checkValidity() { return C.checkValidity(this); }
  reportValidity() { return C.reportValidity(this); }
  setCustomValidity(msg) { W.customValidity(this, String(msg)); }
  showPicker() {}
});
for (const p of ['name', 'placeholder', 'min', 'max', 'step', 'pattern', 'accept', 'alt', 'autocomplete', 'dirName', 'formAction', 'formEnctype', 'formMethod', 'formTarget', 'inputMode', 'src', 'useMap', 'align']) reflectString(classes.HTMLInputElement.prototype, p, p.toLowerCase());
for (const p of ['disabled', 'readOnly', 'required', 'multiple', 'autofocus', 'formNoValidate']) reflectBool(classes.HTMLInputElement.prototype, p, p.toLowerCase());
reflectInt(classes.HTMLInputElement.prototype, 'maxLength', 'maxlength', -1);
reflectInt(classes.HTMLInputElement.prototype, 'minLength', 'minlength', -1);
reflectInt(classes.HTMLInputElement.prototype, 'size', 'size', 20, true);
Object.defineProperty(classes.HTMLInputElement.prototype, 'width', { get() { return this.type === 'image' ? this.offsetWidth : 0; }, set(v) { this.setAttribute('width', String(v | 0)); }, configurable: true });
Object.defineProperty(classes.HTMLInputElement.prototype, 'height', { get() { return this.type === 'image' ? this.offsetHeight : 0; }, set(v) { this.setAttribute('height', String(v | 0)); }, configurable: true });

element('HTMLTextAreaElement', ['textarea'], (B) => class HTMLTextAreaElement extends B {
  get type() { return 'textarea'; }
  get value() { return W.value(this); }
  set value(v) { W.setValue(this, v === null || v === undefined ? '' : String(v)); }
  get defaultValue() { return this.textContent; }
  set defaultValue(v) { this.textContent = v; }
  get textLength() { return this.value.length; }
  get form() { return W.formOwner(this); }
  get selectionStart() { return W.selection(this)[0]; }
  set selectionStart(v) { W.selection(this, +v || 0, Math.max(+v || 0, W.selection(this)[1])); }
  get selectionEnd() { return W.selection(this)[1]; }
  set selectionEnd(v) { W.selection(this, Math.min(W.selection(this)[0], +v || 0), +v || 0); }
  get selectionDirection() { return 'none'; }
  select() { W.selection(this, 0, this.value.length); }
  setSelectionRange(s, e) { W.selection(this, +s || 0, +e || 0); }
  setRangeText(text, start, end) { const v = this.value; if (start === undefined) { const sel = W.selection(this); start = sel[0]; end = sel[1]; } this.value = v.slice(0, start) + text + v.slice(end); }
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
  get willValidate() { return !W.isDisabled(this) && !this.readOnly; }
  get validity() { return C.validityOf(this); }
  get validationMessage() { return C.validationMessageOf(this); }
  checkValidity() { return C.checkValidity(this); }
  reportValidity() { return C.reportValidity(this); }
  setCustomValidity(msg) { W.customValidity(this, String(msg)); }
});
for (const p of ['name', 'placeholder', 'wrap', 'autocomplete', 'dirName']) reflectString(classes.HTMLTextAreaElement.prototype, p, p.toLowerCase());
for (const p of ['disabled', 'readOnly', 'required', 'autofocus']) reflectBool(classes.HTMLTextAreaElement.prototype, p, p.toLowerCase());
reflectInt(classes.HTMLTextAreaElement.prototype, 'rows', 'rows', 2, true);
reflectInt(classes.HTMLTextAreaElement.prototype, 'cols', 'cols', 20, true);
reflectInt(classes.HTMLTextAreaElement.prototype, 'maxLength', 'maxlength', -1);
reflectInt(classes.HTMLTextAreaElement.prototype, 'minLength', 'minlength', -1);

element('HTMLSelectElement', ['select'], (B) => class HTMLSelectElement extends B {
  get type() { return this.multiple ? 'select-multiple' : 'select-one'; }
  get options() { let c = this['%options']; if (!c) { c = W.collection(this, 'options', '', 'HTMLOptionsCollection'); define(this, '%options', c); } return c; }
  get selectedOptions() { return W.collection(this, 'selectedOptions', '', 'HTMLCollection'); }
  get length() { return W.collect(this, 'options', '').length; }
  set length(n) { this.options.length = n; }
  get selectedIndex() { return W.selectedIndex(this); }
  set selectedIndex(v) { W.selectedIndex(this, +v); }
  get value() { return W.value(this); }
  set value(v) { W.setValue(this, v === null || v === undefined ? '' : String(v)); }
  get form() { return W.formOwner(this); }
  item(i) { return this.options.item(i); }
  namedItem(n) { return this.options.namedItem(n); }
  add(el, before) {
    if (typeof before === 'number') before = this.options[before] || null;
    if (before === undefined) before = null;
    const parent = before ? before.parentNode : this;
    W.insertBefore(parent, el, before);
  }
  remove(i) { if (arguments.length === 0) { W.remove(this); return; } const o = this.options[i | 0]; if (o) W.remove(o); }
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
  get willValidate() { return !W.isDisabled(this); }
  get validity() { return C.validityOf(this); }
  get validationMessage() { return C.validationMessageOf(this); }
  checkValidity() { return C.checkValidity(this); }
  reportValidity() { return C.reportValidity(this); }
  setCustomValidity(msg) { W.customValidity(this, String(msg)); }
  showPicker() {}
  [Symbol.iterator]() { return W.collect(this, 'options', '').values(); }
});
for (const p of ['name', 'autocomplete']) reflectString(classes.HTMLSelectElement.prototype, p);
for (const p of ['disabled', 'required', 'multiple', 'autofocus']) reflectBool(classes.HTMLSelectElement.prototype, p);
reflectInt(classes.HTMLSelectElement.prototype, 'size', 'size', 0, true);

element('HTMLOptionElement', ['option'], (B) => class HTMLOptionElement extends B {
  get value() { const v = this.getAttribute('value'); return v === null ? this.text : v; }
  set value(v) { this.setAttribute('value', String(v)); }
  get text() { return this.textContent.replace(/\s+/g, ' ').trim(); }
  set text(v) { this.textContent = v; }
  get label() { const l = this.getAttribute('label'); return l === null ? this.text : l; }
  set label(v) { this.setAttribute('label', String(v)); }
  get selected() { return W.checked(this); }
  set selected(v) { W.setChecked(this, !!v); }
  get defaultSelected() { return this.hasAttribute('selected'); }
  set defaultSelected(v) { if (v) this.setAttribute('selected', ''); else this.removeAttribute('selected'); }
  get index() { const s = this.closest('select'); return s ? W.collect(s, 'options', '').indexOf(this) : 0; }
  get form() { const s = this.closest('select'); return s ? s.form : null; }
});
reflectBool(classes.HTMLOptionElement.prototype, 'disabled');
function Option(text, value, defaultSelected, selected) {
  const el = document.createElement('option');
  if (text !== undefined) el.textContent = String(text);
  if (value !== undefined) el.setAttribute('value', String(value));
  if (defaultSelected) el.setAttribute('selected', '');
  if (selected) W.setChecked(el, true);
  return el;
}
Option.prototype = classes.HTMLOptionElement.prototype;
element('HTMLOptGroupElement', ['optgroup'], (B) => class HTMLOptGroupElement extends B {});
reflectBool(classes.HTMLOptGroupElement.prototype, 'disabled');
reflectString(classes.HTMLOptGroupElement.prototype, 'label');
element('HTMLDataListElement', ['datalist'], (B) => class HTMLDataListElement extends B { get options() { return W.collection(this, 'datalist', '', 'HTMLCollection'); } });

element('HTMLButtonElement', ['button'], (B) => class HTMLButtonElement extends B {
  get type() { const t = (this.getAttribute('type') || '').toLowerCase(); return ['submit', 'reset', 'button'].includes(t) ? t : 'submit'; }
  set type(v) { this.setAttribute('type', String(v)); }
  get form() { return W.formOwner(this); }
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
  get willValidate() { return this.type === 'submit' && !W.isDisabled(this); }
  get validity() { return C.validityOf(this); }
  get validationMessage() { return C.validationMessageOf(this); }
  checkValidity() { return C.checkValidity(this); }
  reportValidity() { return C.reportValidity(this); }
  setCustomValidity(msg) { W.customValidity(this, String(msg)); }
});
for (const p of ['name', 'value', 'formAction', 'formEnctype', 'formMethod', 'formTarget', 'popoverTarget', 'popoverTargetAction']) reflectString(classes.HTMLButtonElement.prototype, p, p.toLowerCase());
for (const p of ['disabled', 'autofocus', 'formNoValidate']) reflectBool(classes.HTMLButtonElement.prototype, p, p.toLowerCase());

element('HTMLFormElement', ['form'], (B) => class HTMLFormElement extends B {
  get elements() { let c = this['%elements']; if (!c) { c = W.collection(this, 'elements', '', 'HTMLFormControlsCollection'); define(this, '%elements', c); } return c; }
  get length() { return W.collect(this, 'elements', '').length; }
  get method() { const m = (this.getAttribute('method') || '').toLowerCase(); return ['get', 'post', 'dialog'].includes(m) ? m : 'get'; }
  set method(v) { this.setAttribute('method', String(v)); }
  get enctype() { const e = (this.getAttribute('enctype') || '').toLowerCase(); return ['application/x-www-form-urlencoded', 'multipart/form-data', 'text/plain'].includes(e) ? e : 'application/x-www-form-urlencoded'; }
  set enctype(v) { this.setAttribute('enctype', String(v)); }
  get encoding() { return this.enctype; }
  set encoding(v) { this.enctype = v; }
  get action() { const a = this.getAttribute('action'); return a === null || a === '' ? document.URL : W.resolveUrl(a, document.baseURI); }
  set action(v) { this.setAttribute('action', String(v)); }
  submit() { hooks.programmaticSubmit(this, null, false); }
  requestSubmit(submitter) {
    if (submitter !== undefined && submitter !== null) {
      if (!(submitter instanceof HTMLElement) || !['button', 'input'].includes(submitter.localName)) throw typeError("Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not a submit button.");
      if (submitter.form !== this) throw domError('NotFoundError', "Failed to execute 'requestSubmit' on 'HTMLFormElement': The specified element is not owned by this form element.");
    }
    hooks.programmaticSubmit(this, submitter || null, true);
  }
  reset() { if (fireSimple(this, 'reset', true, true)) W.resetForm(this); }
  checkValidity() { let ok = true; for (const el of W.collect(this, 'elements', '')) if (el.willValidate && !el.checkValidity()) ok = false; return ok; }
  reportValidity() { let ok = true; for (const el of W.collect(this, 'elements', '')) if (el.willValidate && !el.reportValidity()) ok = false; return ok; }
  get relList() { let c = this['%relList']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'rel'); define(this, '%relList', c); } return c; }
  [Symbol.iterator]() { return W.collect(this, 'elements', '').values(); }
});
for (const p of ['name', 'target', 'acceptCharset', 'autocomplete', 'rel']) reflectString(classes.HTMLFormElement.prototype, p, p === 'acceptCharset' ? 'accept-charset' : p.toLowerCase());
reflectBool(classes.HTMLFormElement.prototype, 'noValidate', 'novalidate');

element('HTMLLabelElement', ['label'], (B) => class HTMLLabelElement extends B {
  get control() { const f = this.getAttribute('for'); if (f !== null) { const el = document.getElementById(f); return el && ['input', 'select', 'textarea', 'button', 'meter', 'output', 'progress'].includes(el.localName) && el.type !== 'hidden' ? el : null; } return this.querySelector('input:not([type=hidden]), select, textarea, button, meter, output, progress'); }
  get form() { const c = this.control; return c ? c.form : null; }
});
reflectString(classes.HTMLLabelElement.prototype, 'htmlFor', 'for');
element('HTMLFieldSetElement', ['fieldset'], (B) => class HTMLFieldSetElement extends B {
  get elements() { return W.collection(this, 'tag', '*', 'HTMLCollection'); }
  get form() { return W.formOwner(this); }
  get type() { return 'fieldset'; }
  get willValidate() { return false; }
  get validity() { return C.validityOf(this); }
  checkValidity() { return true; } reportValidity() { return true; } setCustomValidity() {}
});
reflectBool(classes.HTMLFieldSetElement.prototype, 'disabled');
reflectString(classes.HTMLFieldSetElement.prototype, 'name');
element('HTMLLegendElement', ['legend'], (B) => class HTMLLegendElement extends B { get form() { const f = this.closest('fieldset'); return f ? f.form : null; } });
element('HTMLOutputElement', ['output'], (B) => class HTMLOutputElement extends B {
  get value() { return this.textContent; }
  set value(v) { if (this._dv === undefined) define(this, '_dv', this.textContent); this.textContent = v; }
  get defaultValue() { return this._dv === undefined ? this.textContent : this._dv; }
  set defaultValue(v) { this._dv = String(v); }
  get type() { return 'output'; }
  get form() { return W.formOwner(this); }
  get htmlFor() { let c = this['%for']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'for'); define(this, '%for', c); } return c; }
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
  get willValidate() { return false; }
  get validity() { return C.validityOf(this); }
  checkValidity() { return true; } reportValidity() { return true; } setCustomValidity() {}
});
reflectString(classes.HTMLOutputElement.prototype, 'name');

element('HTMLTemplateElement', ['template'], (B) => class HTMLTemplateElement extends B {
  get content() { return W.templateContent(this); }
});
element('HTMLScriptElement', ['script'], (B) => class HTMLScriptElement extends B {
  get text() { return this.textContent; }
  set text(v) { this.textContent = v; }
  static supports(type) { return type === 'classic' || type === 'module'; }
});
reflectUrl(classes.HTMLScriptElement.prototype, 'src');
for (const p of ['type', 'charset', 'crossOrigin', 'integrity', 'referrerPolicy', 'nonce']) reflectString(classes.HTMLScriptElement.prototype, p, p.toLowerCase());
for (const p of ['async', 'defer', 'noModule']) reflectBool(classes.HTMLScriptElement.prototype, p, p.toLowerCase());

element('HTMLStyleElement', ['style'], (B) => class HTMLStyleElement extends B {
  get sheet() { return C.sheetOfElement(this); }
});
reflectString(classes.HTMLStyleElement.prototype, 'media');
reflectString(classes.HTMLStyleElement.prototype, 'type');
reflectBool(classes.HTMLStyleElement.prototype, 'disabled');
element('HTMLLinkElement', ['link'], (B) => class HTMLLinkElement extends B {
  get sheet() { return C.sheetOfElement(this); }
  get relList() { let c = this['%relList']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'rel'); define(this, '%relList', c); } return c; }
  get sizes() { let c = this['%sizes']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'sizes'); define(this, '%sizes', c); } return c; }
});
reflectUrl(classes.HTMLLinkElement.prototype, 'href');
for (const p of ['rel', 'media', 'type', 'as', 'crossOrigin', 'hreflang', 'integrity', 'referrerPolicy', 'imageSrcset', 'imageSizes']) reflectString(classes.HTMLLinkElement.prototype, p, p.toLowerCase());
reflectBool(classes.HTMLLinkElement.prototype, 'disabled');
C.styleInserted = (el) => { if (el.localName === 'link' && el.hasAttribute('href') && /\bstylesheet\b/i.test(el.getAttribute('rel') || '')) queueTask(() => fireSimple(el, 'load', false, false)); };

element('HTMLIFrameElement', ['iframe'], (B) => class HTMLIFrameElement extends B {
  // Limitation: no nested browsing context; the frame is a box.
  get contentWindow() { return null; }
  get contentDocument() { return null; }
  get sandbox() { let c = this['%sandbox']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'sandbox'); define(this, '%sandbox', c); } return c; }
  getSVGDocument() { return null; }
});
reflectUrl(classes.HTMLIFrameElement.prototype, 'src');
for (const p of ['srcdoc', 'name', 'width', 'height', 'allow', 'loading', 'referrerPolicy', 'align', 'frameBorder', 'scrolling']) reflectString(classes.HTMLIFrameElement.prototype, p, p.toLowerCase());
reflectBool(classes.HTMLIFrameElement.prototype, 'allowFullscreen', 'allowfullscreen');
element('HTMLEmbedElement', ['embed'], (B) => class HTMLEmbedElement extends B {});
element('HTMLObjectElement', ['object'], (B) => class HTMLObjectElement extends B { get contentDocument() { return null; } get contentWindow() { return null; } get form() { return W.formOwner(this); } });

// ---------------------------------------------------------------- canvas

class CanvasGradient {
  constructor(kind, coords) { define(this, '%gradient', kind); this.coords = coords; this.stops = []; }
  addColorStop(offset, color) { offset = +offset; if (!(offset >= 0 && offset <= 1)) throw domError('IndexSizeError', 'The provided value (' + offset + ') is outside the range (0.0, 1.0).'); this.stops.push([offset, String(color)]); }
}
class CanvasPattern { constructor() {} setTransform() {} }
class ImageData {
  constructor(a, b, c) {
    if (typeof a === 'object') { this.data = a; this.width = b | 0; this.height = c === undefined ? (a.length / 4 / b) | 0 : c | 0; }
    else { this.width = a | 0; this.height = b | 0; if (this.width <= 0 || this.height <= 0) throw domError('IndexSizeError', 'The source width is zero or not a number.'); this.data = new Uint8ClampedArray(this.width * this.height * 4); }
    this.colorSpace = 'srgb';
  }
}
class TextMetrics { constructor(w, size) { this.width = w; this.actualBoundingBoxLeft = 0; this.actualBoundingBoxRight = w; this.actualBoundingBoxAscent = size * 0.75; this.actualBoundingBoxDescent = size * 0.25; this.fontBoundingBoxAscent = size * 0.9; this.fontBoundingBoxDescent = size * 0.25; this.emHeightAscent = size * 0.8; this.emHeightDescent = size * 0.2; this.alphabeticBaseline = 0; this.hangingBaseline = size * 0.8; this.ideographicBaseline = -size * 0.2; } }
class Path2D {
  constructor(path) { this._ops = []; if (path instanceof Path2D) this._ops = path._ops.slice(); }
  _add(op, args) { this._ops.push([op, args]); }
  moveTo(...a) { this._add('moveTo', a); } lineTo(...a) { this._add('lineTo', a); } rect(...a) { this._add('rect', a); } arc(...a) { this._add('arc', a); } arcTo(...a) { this._add('arcTo', a); } ellipse(...a) { this._add('ellipse', a); } closePath() { this._add('closePath', []); } quadraticCurveTo(...a) { this._add('quadraticCurveTo', a); } bezierCurveTo(...a) { this._add('bezierCurveTo', a); } roundRect(...a) { this._add('rect', a.slice(0, 4)); }
  addPath(p) { this._ops.push(...p._ops); }
}

const canvasOps = ['fillRect', 'strokeRect', 'clearRect', 'beginPath', 'closePath', 'moveTo', 'lineTo', 'rect', 'arc', 'ellipse', 'arcTo', 'quadraticCurveTo', 'bezierCurveTo', 'stroke', 'clip', 'save', 'restore', 'translate', 'scale', 'rotate', 'transform', 'setTransform', 'resetTransform'];
const canvasStateProps = ['fillStyle', 'strokeStyle', 'lineWidth', 'globalAlpha', 'font', 'textAlign', 'textBaseline', 'lineCap', 'lineJoin', 'globalCompositeOperation', 'imageSmoothingEnabled', 'miterLimit', 'shadowBlur', 'shadowColor'];
class CanvasRenderingContext2D {
  constructor(canvas) { if (!canvas) throw typeError('Illegal constructor'); define(this, '_canvas', canvas); this._dash = []; this.lineDashOffset = 0; this.shadowOffsetX = 0; this.shadowOffsetY = 0; this.filter = 'none'; this.direction = 'ltr'; this.fontKerning = 'auto'; this.letterSpacing = '0px'; this.wordSpacing = '0px'; this.imageSmoothingQuality = 'low'; }
  get canvas() { return this._canvas; }
  _replay(path) { W.canvasOp(this._canvas, 'beginPath'); for (const [op, args] of path._ops) W.canvasOp(this._canvas, op, ...args); }
  fill(a, b) { if (a instanceof Path2D) { this._replay(a); W.canvasOp(this._canvas, 'fill', b); } else W.canvasOp(this._canvas, 'fill', a); }
  isPointInPath(a, b, c, d) { if (a instanceof Path2D) { this._replay(a); return W.canvasOp(this._canvas, 'isPointInPath', b, c, d); } return W.canvasOp(this._canvas, 'isPointInPath', a, b, c); }
  isPointInStroke() { return false; }
  fillText(text, x, y, maxWidth) { W.canvasOp(this._canvas, 'fillText', String(text), +x, +y, maxWidth); }
  strokeText(text, x, y, maxWidth) { W.canvasOp(this._canvas, 'strokeText', String(text), +x, +y, maxWidth); }
  measureText(text) { return new TextMetrics(W.canvasOp(this._canvas, 'measureText', String(text)), parseFloat(this.font) || 10); }
  roundRect(x, y, w, h) { W.canvasOp(this._canvas, 'rect', x, y, w, h); }
  createLinearGradient(x0, y0, x1, y1) { return new CanvasGradient('linear', [+x0, +y0, +x1, +y1]); }
  createRadialGradient(x0, y0, r0, x1, y1, r1) { return new CanvasGradient('radial', [+x0, +y0, +r0, +x1, +y1, +r1]); }
  createConicGradient(angle, x, y) { return new CanvasGradient('radial', [+x, +y, 0, +x, +y, 100]); }
  createPattern() { return new CanvasPattern(); }
  setLineDash(d) { this._dash = Array.from(d); } getLineDash() { return this._dash.slice(); }
  drawImage(img, ...args) {
    // Only other canvases carry pixels; images have none in this realm.
    const src = img && img.localName === 'canvas' ? img : null;
    if (!src) return;
    const sw = W.canvasState(src, 'width'), sh = W.canvasState(src, 'height');
    let sx = 0, sy = 0, srw = sw, srh = sh, dx, dy, dw, dh;
    if (args.length <= 4) { [dx, dy, dw, dh] = args; if (dw === undefined) { dw = sw; dh = sh; } }
    else { [sx, sy, srw, srh, dx, dy, dw, dh] = args; }
    W.canvasOp(this._canvas, 'drawImage', src, +sx, +sy, +srw, +srh, +dx, +dy, +dw, +dh);
  }
  getImageData(x, y, w, h) { const data = W.canvasGetImageData(this._canvas, x | 0, y | 0, w | 0, h | 0); return new ImageData(data, w | 0, h | 0); }
  putImageData(img, x, y) { W.canvasPutImageData(this._canvas, img.data, img.width, img.height, x | 0, y | 0); }
  createImageData(w, h) { if (typeof w === 'object') return new ImageData(w.width, w.height); return new ImageData(w, h); }
  getTransform() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }; }
  getContextAttributes() { return { alpha: true, desynchronized: false, colorSpace: 'srgb', willReadFrequently: false }; }
  reset() { W.canvasOp(this._canvas, 'resize', W.canvasState(this._canvas, 'width'), W.canvasState(this._canvas, 'height')); }
  drawFocusIfNeeded() {} scrollPathIntoView() {}
}
for (const op of canvasOps) define(CanvasRenderingContext2D.prototype, op, function (...args) { W.canvasOp(this._canvas, op, ...args); });
for (const p of canvasStateProps) Object.defineProperty(CanvasRenderingContext2D.prototype, p, { get() { return W.canvasState(this._canvas, p); }, set(v) { W.canvasSetState(this._canvas, p, v); }, configurable: true, enumerable: true });

element('HTMLCanvasElement', ['canvas'], (B) => class HTMLCanvasElement extends B {
  get width() { const w = parseInt(this.getAttribute('width'), 10); return Number.isNaN(w) || w < 0 ? 300 : w; }
  set width(v) { this.setAttribute('width', String(Math.max(0, v | 0))); }
  get height() { const h = parseInt(this.getAttribute('height'), 10); return Number.isNaN(h) || h < 0 ? 150 : h; }
  set height(v) { this.setAttribute('height', String(Math.max(0, v | 0))); }
  getContext(kind) {
    if (kind === '2d') { let c = this['%ctx']; if (!c) { c = new CanvasRenderingContext2D(this); define(this, '%ctx', c); } return c; }
    return null;
  }
  toDataURL() { return W.canvasOp(this._canvasSelf || this, 'toDataURL'); }
  toBlob(cb, type) { const url = this.toDataURL(); const b = new Blob([atob(url.split(',')[1])], { type: 'image/png' }); queueTask(() => cb(b)); }
  captureStream() { return null; }
  transferControlToOffscreen() { throw domError('InvalidStateError', 'Not supported.'); }
});

// ---------------------------------------------------------------- media, tables, misc

element('HTMLMediaElement', [], (B) => class HTMLMediaElement extends B {
  constructor() { super(); }
  get paused() { return true; } get ended() { return false; } get duration() { return NaN; } get readyState() { return 0; } get networkState() { return 0; } get seeking() { return false; } get buffered() { return { length: 0, start() { return 0; }, end() { return 0; } }; } get played() { return this.buffered; } get seekable() { return this.buffered; } get error() { return null; } get textTracks() { return []; }
  get currentTime() { return this._ct || 0; } set currentTime(v) { this._ct = +v || 0; }
  get muted() { return this._muted !== undefined ? this._muted : this.hasAttribute('muted'); } set muted(v) { this._muted = !!v; }
  get volume() { return this._vol === undefined ? 1 : this._vol; } set volume(v) { this._vol = +v; }
  get playbackRate() { return this._rate === undefined ? 1 : this._rate; } set playbackRate(v) { this._rate = +v; }
  get defaultPlaybackRate() { return 1; } set defaultPlaybackRate(v) {}
  get currentSrc() { return this.src; }
  play() { return Promise.reject(domError('NotSupportedError', 'The element has no supported sources.')); }
  pause() {} load() {} canPlayType() { return ''; } addTextTrack() { return { addCue() {}, cues: [] }; } fastSeek() {}
});
reflectUrl(classes.HTMLMediaElement.prototype, 'src');
for (const p of ['preload', 'crossOrigin']) reflectString(classes.HTMLMediaElement.prototype, p, p.toLowerCase());
for (const p of ['autoplay', 'loop', 'controls', 'defaultMuted']) reflectBool(classes.HTMLMediaElement.prototype, p, p === 'defaultMuted' ? 'muted' : p);
element('HTMLVideoElement', ['video'], () => class HTMLVideoElement extends classes.HTMLMediaElement { get videoWidth() { return 0; } get videoHeight() { return 0; } requestPictureInPicture() { return Promise.reject(domError('NotSupportedError', 'Not supported.')); } });
reflectString(classes.HTMLVideoElement.prototype, 'poster');
reflectInt(classes.HTMLVideoElement.prototype, 'width', 'width', 0);
reflectInt(classes.HTMLVideoElement.prototype, 'height', 'height', 0);
element('HTMLAudioElement', ['audio'], () => class HTMLAudioElement extends classes.HTMLMediaElement {});
function Audio(src) { const el = document.createElement('audio'); if (src !== undefined) el.setAttribute('src', String(src)); el.setAttribute('preload', 'auto'); return el; }
Audio.prototype = classes.HTMLAudioElement.prototype;
element('HTMLSourceElement', ['source'], (B) => class HTMLSourceElement extends B {});
reflectUrl(classes.HTMLSourceElement.prototype, 'src');
for (const p of ['type', 'srcset', 'sizes', 'media']) reflectString(classes.HTMLSourceElement.prototype, p);
element('HTMLTrackElement', ['track'], (B) => class HTMLTrackElement extends B { get readyState() { return 0; } get track() { return { mode: 'disabled', cues: [] }; } });

element('HTMLTableElement', ['table'], (B) => class HTMLTableElement extends B {
  get rows() { return W.collection(this, 'rows', '', 'HTMLCollection'); }
  get tBodies() { return W.collection(this, 'tBodies', '', 'HTMLCollection'); }
  get caption() { return this.querySelector(':scope > caption'); }
  set caption(v) { const old = this.caption; if (old) W.remove(old); if (v) W.insertBefore(this, v, this.firstChild); }
  get tHead() { return this.querySelector(':scope > thead'); }
  set tHead(v) { const old = this.tHead; if (old) W.remove(old); if (v) W.insertBefore(this, v, this.querySelector(':scope > tbody, :scope > tfoot, :scope > tr')); }
  get tFoot() { return this.querySelector(':scope > tfoot'); }
  set tFoot(v) { const old = this.tFoot; if (old) W.remove(old); if (v) W.insertBefore(this, v, null); }
  createCaption() { let c = this.caption; if (!c) { c = document.createElement('caption'); W.insertBefore(this, c, this.firstChild); } return c; }
  deleteCaption() { const c = this.caption; if (c) W.remove(c); }
  createTHead() { let t = this.tHead; if (!t) { t = document.createElement('thead'); this.tHead = t; } return t; }
  deleteTHead() { const t = this.tHead; if (t) W.remove(t); }
  createTFoot() { let t = this.tFoot; if (!t) { t = document.createElement('tfoot'); W.insertBefore(this, t, null); } return t; }
  deleteTFoot() { const t = this.tFoot; if (t) W.remove(t); }
  createTBody() { const t = document.createElement('tbody'); const last = Array.from(this.children).filter((c) => c.localName === 'tbody').pop(); W.insertBefore(this, t, last ? last.nextSibling : null); return t; }
  insertRow(index) {
    const rows = W.collect(this, 'rows', '');
    if (index === undefined) index = -1;
    if (index < -1 || index > rows.length) throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + rows.length + '].');
    const tr = document.createElement('tr');
    if (rows.length === 0) { let tb = Array.from(this.children).filter((c) => c.localName === 'tbody').pop(); if (!tb) tb = this.createTBody(); W.insertBefore(tb, tr, null); }
    else if (index === -1 || index === rows.length) { const last = rows[rows.length - 1]; W.insertBefore(last.parentNode, tr, null); }
    else { const r = rows[index]; W.insertBefore(r.parentNode, tr, r); }
    return tr;
  }
  deleteRow(index) {
    const rows = W.collect(this, 'rows', '');
    if (index === -1) index = rows.length - 1;
    if (index < 0 || index >= rows.length) { if (rows.length === 0 && index === -1) return; throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + rows.length + ').'); }
    W.remove(rows[index]);
  }
});
for (const p of ['align', 'border', 'frame', 'rules', 'summary', 'width', 'bgColor', 'cellPadding', 'cellSpacing']) reflectString(classes.HTMLTableElement.prototype, p, p.toLowerCase());
element('HTMLTableSectionElement', ['thead', 'tbody', 'tfoot'], (B) => class HTMLTableSectionElement extends B {
  get rows() { return W.collection(this, 'rows', '', 'HTMLCollection'); }
  insertRow(index) { const rows = W.collect(this, 'rows', ''); if (index === undefined) index = -1; if (index < -1 || index > rows.length) throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + rows.length + '].'); const tr = document.createElement('tr'); W.insertBefore(this, tr, index === -1 || index === rows.length ? null : rows[index]); return tr; }
  deleteRow(index) { const rows = W.collect(this, 'rows', ''); if (index === -1) index = rows.length - 1; if (index < 0 || index >= rows.length) { if (rows.length === 0) return; throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + rows.length + ').'); } W.remove(rows[index]); }
});
element('HTMLTableRowElement', ['tr'], (B) => class HTMLTableRowElement extends B {
  get cells() { return W.collection(this, 'cells', '', 'HTMLCollection'); }
  get rowIndex() { const t = this.closest('table'); return t ? W.collect(t, 'rows', '').indexOf(this) : -1; }
  get sectionRowIndex() { const p = this.parentNode; return p ? W.collect(p, 'rows', '').indexOf(this) : -1; }
  insertCell(index) { const cells = W.collect(this, 'cells', ''); if (index === undefined) index = -1; if (index < -1 || index > cells.length) throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + cells.length + '].'); const td = document.createElement('td'); W.insertBefore(this, td, index === -1 || index === cells.length ? null : cells[index]); return td; }
  deleteCell(index) { const cells = W.collect(this, 'cells', ''); if (index === -1) index = cells.length - 1; if (index < 0 || index >= cells.length) { if (cells.length === 0) return; throw domError('IndexSizeError', 'The provided index (' + index + ') is outside the range [-1, ' + cells.length + ').'); } W.remove(cells[index]); }
});
element('HTMLTableCellElement', ['td', 'th'], (B) => class HTMLTableCellElement extends B {
  get cellIndex() { const p = this.parentNode; return p && p.localName === 'tr' ? W.collect(p, 'cells', '').indexOf(this) : -1; }
});
reflectInt(classes.HTMLTableCellElement.prototype, 'colSpan', 'colspan', 1);
reflectInt(classes.HTMLTableCellElement.prototype, 'rowSpan', 'rowspan', 1);
for (const p of ['headers', 'abbr', 'scope', 'align', 'vAlign', 'width', 'height', 'bgColor']) reflectString(classes.HTMLTableCellElement.prototype, p, p.toLowerCase());
element('HTMLTableColElement', ['col', 'colgroup'], (B) => class HTMLTableColElement extends B {});
reflectInt(classes.HTMLTableColElement.prototype, 'span', 'span', 1);
element('HTMLTableCaptionElement', ['caption'], (B) => class HTMLTableCaptionElement extends B {});

element('HTMLDetailsElement', ['details'], (B) => class HTMLDetailsElement extends B {});
Object.defineProperty(classes.HTMLDetailsElement.prototype, 'open', { get() { return this.hasAttribute('open'); }, set(v) { const was = this.hasAttribute('open'); if (v) this.setAttribute('open', ''); else this.removeAttribute('open'); if (was !== !!v) queueTask(() => fire(this, new C.ToggleEvent('toggle', { oldState: was ? 'open' : 'closed', newState: v ? 'open' : 'closed' }))); }, configurable: true, enumerable: true });
reflectString(classes.HTMLDetailsElement.prototype, 'name');
element('HTMLDialogElement', ['dialog'], (B) => class HTMLDialogElement extends B {
  constructor() { super(); }
  get open() { return this.hasAttribute('open'); }
  set open(v) { if (v) this.setAttribute('open', ''); else this.removeAttribute('open'); }
  get returnValue() { return this._rv || ''; }
  set returnValue(v) { this._rv = String(v); }
  show() { if (this.open) return; this.setAttribute('open', ''); this._focusFirst(); }
  showModal() { if (this.open) throw domError('InvalidStateError', "The element already has an 'open' attribute, and therefore cannot be opened modally."); if (!this.isConnected) throw domError('InvalidStateError', 'The element is not in a Document.'); this.setAttribute('open', ''); define(this, '_modal', true); this._focusFirst(); }
  close(rv) { if (!this.open) return; if (rv !== undefined) this._rv = String(rv); this.removeAttribute('open'); define(this, '_modal', false); queueTask(() => fireSimple(this, 'close', false, false)); }
  requestClose(rv) { if (fireSimple(this, 'cancel', false, true)) this.close(rv); }
  _focusFirst() { const f = this.querySelector('[autofocus], input, button, select, textarea, a[href]'); if (f && f.focus) f.focus(); }
});
element('HTMLMeterElement', ['meter'], (B) => class HTMLMeterElement extends B {
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
});
for (const [p, d] of [['value', 0], ['min', 0], ['max', 1], ['low', 0], ['high', 1], ['optimum', 0.5]]) Object.defineProperty(classes.HTMLMeterElement.prototype, p, { get() { const v = parseFloat(this.getAttribute(p)); if (Number.isNaN(v)) { if (p === 'max') return Math.max(1, this.min); if (p === 'low') return this.min; if (p === 'high') return this.max; if (p === 'optimum') return (this.min + this.max) / 2; return p === 'value' ? Math.min(Math.max(0, this.min), this.max) : d; } return v; }, set(v) { this.setAttribute(p, String(+v)); }, configurable: true, enumerable: true });
element('HTMLProgressElement', ['progress'], (B) => class HTMLProgressElement extends B {
  get value() { const v = parseFloat(this.getAttribute('value')); return Number.isNaN(v) ? 0 : Math.min(Math.max(0, v), this.max); }
  set value(v) { this.setAttribute('value', String(+v)); }
  get max() { const v = parseFloat(this.getAttribute('max')); return Number.isNaN(v) || v <= 0 ? 1 : v; }
  set max(v) { if (+v > 0) this.setAttribute('max', String(+v)); }
  get position() { return this.hasAttribute('value') ? this.value / this.max : -1; }
  get labels() { return W.collection(this, 'labels', '', 'NodeList'); }
});
element('HTMLOListElement', ['ol'], (B) => class HTMLOListElement extends B {});
reflectInt(classes.HTMLOListElement.prototype, 'start', 'start', 1);
reflectBool(classes.HTMLOListElement.prototype, 'reversed');
reflectString(classes.HTMLOListElement.prototype, 'type');
element('HTMLUListElement', ['ul'], (B) => class HTMLUListElement extends B {});
element('HTMLLIElement', ['li'], (B) => class HTMLLIElement extends B {});
reflectInt(classes.HTMLLIElement.prototype, 'value', 'value', 0);
element('HTMLTimeElement', ['time'], (B) => class HTMLTimeElement extends B {});
reflectString(classes.HTMLTimeElement.prototype, 'dateTime', 'datetime');
element('HTMLDataElement', ['data'], (B) => class HTMLDataElement extends B {});
reflectString(classes.HTMLDataElement.prototype, 'value');
element('HTMLSlotElement', ['slot'], (B) => class HTMLSlotElement extends B { assignedNodes() { return []; } assignedElements() { return []; } assign() {} });
reflectString(classes.HTMLSlotElement.prototype, 'name');
element('HTMLDivElement', ['div'], (B) => class HTMLDivElement extends B {});
reflectString(classes.HTMLDivElement.prototype, 'align');
element('HTMLSpanElement', ['span'], (B) => class HTMLSpanElement extends B {});
element('HTMLParagraphElement', ['p'], (B) => class HTMLParagraphElement extends B {});
element('HTMLHeadingElement', ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'], (B) => class HTMLHeadingElement extends B {});
element('HTMLBRElement', ['br'], (B) => class HTMLBRElement extends B {});
element('HTMLHRElement', ['hr'], (B) => class HTMLHRElement extends B {});
element('HTMLPreElement', ['pre', 'listing', 'xmp'], (B) => class HTMLPreElement extends B {});
element('HTMLQuoteElement', ['blockquote', 'q'], (B) => class HTMLQuoteElement extends B {});
reflectUrl(classes.HTMLQuoteElement.prototype, 'cite');
element('HTMLModElement', ['ins', 'del'], (B) => class HTMLModElement extends B {});
reflectString(classes.HTMLModElement.prototype, 'dateTime', 'datetime');
element('HTMLBodyElement', ['body'], (B) => class HTMLBodyElement extends B {});
for (const p of ['bgColor', 'text', 'link', 'vLink', 'aLink', 'background']) reflectString(classes.HTMLBodyElement.prototype, p, p.toLowerCase());
for (const t of C.windowEventTypes.concat(['blur', 'error', 'focus', 'load', 'resize', 'scroll'])) Object.defineProperty(classes.HTMLBodyElement.prototype, 'on' + t, { get() { return globalThis['on' + t]; }, set(v) { globalThis['on' + t] = v; }, configurable: true, enumerable: true });
element('HTMLHtmlElement', ['html'], (B) => class HTMLHtmlElement extends B {});
element('HTMLHeadElement', ['head'], (B) => class HTMLHeadElement extends B {});
element('HTMLTitleElement', ['title'], (B) => class HTMLTitleElement extends B { get text() { return this.textContent; } set text(v) { this.textContent = v; } });
element('HTMLMetaElement', ['meta'], (B) => class HTMLMetaElement extends B {});
for (const p of ['name', 'content', 'httpEquiv', 'media', 'scheme', 'charset']) reflectString(classes.HTMLMetaElement.prototype, p, p === 'httpEquiv' ? 'http-equiv' : p.toLowerCase());
element('HTMLBaseElement', ['base'], (B) => class HTMLBaseElement extends B {});
reflectUrl(classes.HTMLBaseElement.prototype, 'href');
reflectString(classes.HTMLBaseElement.prototype, 'target');
element('HTMLMenuElement', ['menu'], (B) => class HTMLMenuElement extends B {});
element('HTMLDListElement', ['dl'], (B) => class HTMLDListElement extends B {});
element('HTMLPictureElement', ['picture'], (B) => class HTMLPictureElement extends B {});
element('HTMLMapElement', ['map'], (B) => class HTMLMapElement extends B { get areas() { return W.collection(this, 'areas', '', 'HTMLCollection'); } });
element('HTMLFontElement', ['font'], (B) => class HTMLFontElement extends B {});
for (const p of ['color', 'face', 'size']) reflectString(classes.HTMLFontElement.prototype, p);
element('HTMLMarqueeElement', ['marquee'], (B) => class HTMLMarqueeElement extends B { start() {} stop() {} });
element('HTMLFrameSetElement', ['frameset'], (B) => class HTMLFrameSetElement extends B {});
element('HTMLFrameElement', ['frame'], (B) => class HTMLFrameElement extends B { get contentWindow() { return null; } get contentDocument() { return null; } });
element('HTMLParamElement', ['param'], (B) => class HTMLParamElement extends B {});
element('HTMLDirectoryElement', ['dir'], (B) => class HTMLDirectoryElement extends B {});
element('HTMLSearchElement', ['search'], (B) => class HTMLSearchElement extends B {});

// Generic elements the spec gives HTMLElement itself.
for (const t of ['abbr', 'address', 'article', 'aside', 'b', 'bdi', 'bdo', 'cite', 'code', 'dd', 'dfn', 'dt', 'em', 'figcaption', 'figure', 'footer', 'header', 'hgroup', 'i', 'kbd', 'main', 'mark', 'nav', 'noscript', 'rp', 'rt', 'ruby', 's', 'samp', 'section', 'small', 'strong', 'sub', 'summary', 'sup', 'u', 'var', 'wbr', 'center', 'nobr', 'tt', 'big', 'strike', 'acronym', 'noframes', 'noembed', 'plaintext', 'selectedcontent']) tags[t] = HTMLElement;

// ---------------------------------------------------------------- validation

function validityOf(el) {
  const v = { valueMissing: false, typeMismatch: false, patternMismatch: false, tooLong: false, tooShort: false, rangeUnderflow: false, rangeOverflow: false, stepMismatch: false, badInput: false, customError: false, valid: true };
  if (!el.willValidate) return v;
  const tag = el.localName;
  const type = tag === 'input' ? el.type : tag;
  const value = tag === 'select' ? el.value : (el.value || '');
  if (W.customValidity(el)) v.customError = true;
  if (el.hasAttribute('required')) {
    if (type === 'checkbox') v.valueMissing = !el.checked;
    else if (type === 'radio') { const form = el.form; const group = (form ? W.collect(form, 'elements', '') : Array.from(document.querySelectorAll('input[type=radio]'))).filter((r) => r.localName === 'input' && r.type === 'radio' && r.name === el.name && r.name); v.valueMissing = !group.some((r) => r.checked); }
    else if (type === 'select') v.valueMissing = value === '' || (el.selectedIndex >= 0 && el.options[el.selectedIndex] && el.options[el.selectedIndex].hasAttribute('placeholder')) ? value === '' : false;
    else if (type === 'file') v.valueMissing = el.files.length === 0;
    else v.valueMissing = value === '';
  }
  if (value !== '' && tag === 'input') {
    if (type === 'email') { const re = /^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/; v.typeMismatch = !(el.multiple ? value.split(',').every((p) => re.test(p.trim())) : re.test(value)); }
    if (type === 'url') { try { new URL(value); } catch (e) { v.typeMismatch = true; } }
    const pattern = el.getAttribute('pattern');
    if (pattern !== null && ['text', 'search', 'url', 'tel', 'email', 'password'].includes(type)) { try { const re = new RegExp('^(?:' + pattern + ')$', 'u'); v.patternMismatch = !(el.multiple && type === 'email' ? value.split(',').every((p) => re.test(p.trim())) : re.test(value)); } catch (e) { /* invalid pattern: valid */ } }
    if (type === 'number' || type === 'range' || type === 'date' || type === 'time' || type === 'month' || type === 'week' || type === 'datetime-local') {
      const num = type === 'number' || type === 'range' ? Number(value) : el.valueAsNumber;
      const conv = (s) => (type === 'number' || type === 'range' ? parseFloat(s) : (() => { const t = document.createElement('input'); t.type = type; t.value = s; return t.valueAsNumber; })());
      if (Number.isNaN(num)) v.badInput = true;
      else {
        const min = el.getAttribute('min'), max = el.getAttribute('max');
        if (min !== null && !Number.isNaN(conv(min)) && num < conv(min)) v.rangeUnderflow = true;
        if (max !== null && !Number.isNaN(conv(max)) && num > conv(max)) v.rangeOverflow = true;
        const stepAttr = el.getAttribute('step');
        if (stepAttr !== 'any' && (type === 'number' || type === 'range')) {
          const step = stepAttr === null ? 1 : parseFloat(stepAttr);
          const base = min !== null && !Number.isNaN(parseFloat(min)) ? parseFloat(min) : 0;
          if (step > 0) { const q = Math.abs(num - base) / step; if (Math.abs(q - Math.round(q)) > 1e-9) v.stepMismatch = true; }
        }
      }
    }
  }
  if (W.valueDirty(el) && (tag === 'textarea' || ['text', 'search', 'url', 'tel', 'email', 'password'].includes(type))) {
    const maxLength = el.maxLength, minLength = el.minLength;
    if (maxLength >= 0 && value.length > maxLength) v.tooLong = true;
    if (minLength >= 0 && value !== '' && value.length < minLength) v.tooShort = true;
  }
  v.valid = !(v.valueMissing || v.typeMismatch || v.patternMismatch || v.tooLong || v.tooShort || v.rangeUnderflow || v.rangeOverflow || v.stepMismatch || v.badInput || v.customError);
  return v;
}
function validationMessageOf(el) {
  const v = validityOf(el);
  if (v.valid) return '';
  if (v.customError) return W.customValidity(el);
  if (v.valueMissing) return el.localName === 'select' ? 'Please select an item in the list.' : el.type === 'checkbox' ? 'Please check this box if you want to proceed.' : el.type === 'radio' ? 'Please select one of these options.' : el.type === 'file' ? 'Please select a file.' : 'Please fill out this field.';
  if (v.typeMismatch) return el.type === 'email' ? "Please include an '@' in the email address. '" + el.value + "' is missing an '@'." : 'Please enter a URL.';
  if (v.patternMismatch) return el.title ? el.title : 'Please match the requested format.';
  if (v.tooLong) return 'Please shorten this text to ' + el.maxLength + ' characters or less (you are currently using ' + el.value.length + ' characters).';
  if (v.tooShort) return 'Please lengthen this text to ' + el.minLength + ' characters or more (you are currently using ' + el.value.length + ' characters).';
  if (v.rangeUnderflow) return 'Value must be greater than or equal to ' + el.getAttribute('min') + '.';
  if (v.rangeOverflow) return 'Value must be less than or equal to ' + el.getAttribute('max') + '.';
  if (v.stepMismatch) return 'Please enter a valid value.';
  if (v.badInput) return 'Please enter a number.';
  return 'Invalid value.';
}
function checkValidity(el) {
  if (validityOf(el).valid) return true;
  fireSimple(el, 'invalid', false, true);
  return false;
}
function reportValidity(el) {
  if (validityOf(el).valid) return true;
  const notPrevented = fireSimple(el, 'invalid', false, true);
  if (notPrevented && el.focus) el.focus();
  return false;
}
hooks.validate = (form) => {
  let ok = true;
  for (const el of W.collect(form, 'elements', '')) if (el.willValidate && !validityOf(el).valid) { ok = false; if (fireSimple(el, 'invalid', false, true) && ok === false) { /* browser would show a bubble */ } }
  return ok;
};

class FileList { constructor(files) { for (let i = 0; i < files.length; i++) this[i] = files[i]; define(this, 'length', files.length); } item(i) { return this[i] || null; } [Symbol.iterator]() { return Array.prototype.values.call(this); } }

Object.assign(C, { classes, tags, Image, Option, Audio, CanvasRenderingContext2D, CanvasGradient, CanvasPattern, ImageData, TextMetrics, Path2D, FileList, validityOf, validationMessageOf, checkValidity, reportValidity, defineUrlParts });
})();
