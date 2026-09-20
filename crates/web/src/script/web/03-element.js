// Prelude part 3: Element, HTMLElement, Document, fragments, shadow roots and the
// custom element registry.
(function () {
'use strict';
const C = globalThis['%core'];
const { W, hooks, define, isNode, domError, typeError, SHADOW_HOST, Node, mixin, parentNodeMethods, childNodeMethods, nodeList, rectOf, DOMRect, DOMRectList, defineHandlerAttr, globalEventTypes, documentEventTypes, fire, fireSimple, Range, TreeWalker, NodeIterator, selection, Text, Comment, DocumentType } = C;

// ---------------------------------------------------------------- Element

class Element extends Node {
  constructor() { super(); }
  get prefix() { return null; }
  get classList() {
    let c = this['%classList'];
    if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'class'); define(this, '%classList', c); }
    return c;
  }
  get part() { let c = this['%part']; if (!c) { c = W.elementPart(this, 'tokens', 'DOMTokenList', 'part'); define(this, '%part', c); } return c; }
  get attributes() {
    let c = this['%attributes'];
    if (!c) { c = W.elementPart(this, 'attrs', 'NamedNodeMap'); define(this, '%attributes', c); }
    return c;
  }
  get slot() { return this.getAttribute('slot') || ''; }
  set slot(v) { this.setAttribute('slot', v); }
  get shadowRoot() { const r = this['%shadow']; return r && r._mode === 'open' ? r : null; }
  attachShadow(init) {
    if (!init || (init.mode !== 'open' && init.mode !== 'closed')) throw typeError("Failed to execute 'attachShadow' on 'Element': The provided value is not of type 'ShadowRootMode'.");
    if (this['%shadow']) throw domError('NotSupportedError', 'Shadow root cannot be created on a host which already hosts a shadow tree.');
    // Approximation: the shadow tree is a fragment rendered as light content: its
    // nodes live under the host, and styles are not scoped.
    const root = W.createFragment();
    W.setProto(root, ShadowRoot.prototype);
    define(root, '_mode', init.mode);
    define(root, '_host', this);
    define(root, SHADOW_HOST, this);
    define(root, '_delegatesFocus', !!init.delegatesFocus);
    define(this, '%shadow', root);
    return root;
  }
  getAttributeNS(ns, name) { return this.getAttribute(name); }
  setAttributeNS(ns, name, value) { this.setAttribute(name, value); }
  removeAttributeNS(ns, name) { this.removeAttribute(name); }
  hasAttributeNS(ns, name) { return this.hasAttribute(name); }
  getAttributeNode(name) { return this.hasAttribute(name) ? C.attrNode(this, String(name).toLowerCase()) : null; }
  getAttributeNodeNS(ns, name) { return this.getAttributeNode(name); }
  setAttributeNode(attr) { return this.attributes.setNamedItem(attr); }
  setAttributeNodeNS(attr) { return this.attributes.setNamedItem(attr); }
  removeAttributeNode(attr) { return this.attributes.removeNamedItem(attr.name); }
  matches(sel) { return W.matches(this, sel); }
  webkitMatchesSelector(sel) { return W.matches(this, sel); }
  msMatchesSelector(sel) { return W.matches(this, sel); }
  closest(sel) { return W.closest(this, sel); }
  get innerHTML() { return W.innerHTML(this); }
  set innerHTML(v) { W.setInnerHTML(this, v); }
  get outerHTML() { return W.outerHTML(this); }
  set outerHTML(v) {
    const p = this.parentNode;
    if (!p) return;
    const frag = W.parseFragment(p.nodeType === 1 ? p : (document.body || document.documentElement), String(v));
    W.insertBefore(p, frag, this.nextSibling);
    W.remove(this);
  }
  insertAdjacentHTML(position, html) {
    const pos = String(position).toLowerCase();
    if (!['beforebegin', 'afterbegin', 'beforeend', 'afterend'].includes(pos)) throw domError('SyntaxError', "The value provided ('" + position + "') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'.");
    const ctx = pos === 'beforebegin' || pos === 'afterend' ? this.parentNode : this;
    if (!ctx) throw domError('NoModificationAllowedError', 'The element has no parent.');
    const frag = W.parseFragment(ctx.nodeType === 1 ? ctx : (document.body || this), String(html));
    W.insertAdjacent(this, pos, frag);
  }
  insertAdjacentElement(position, el) { return W.insertAdjacent(this, position, el); }
  insertAdjacentText(position, text) { W.insertAdjacent(this, position, document.createTextNode(String(text))); }
  getBoundingClientRect() { return rectOf(W.boundingRect(this)); }
  getClientRects() { return new DOMRectList(W.clientRects(this).map(rectOf)); }
  scrollIntoView(arg) {
    const top = !(arg === false || (arg && typeof arg === 'object' && arg.block === 'end'));
    const block = arg && typeof arg === 'object' ? arg.block : undefined;
    W.scrollIntoView(this, top, block === 'center' ? 'center' : '');
  }
  scrollIntoViewIfNeeded() { W.scrollIntoView(this, true, 'center'); }
  scrollTo(x, y) { if (typeof x === 'object' && x !== null) { y = x.top; x = x.left; } W.scrollTo(this, x === undefined ? null : x, y === undefined ? null : y); }
  scroll(x, y) { this.scrollTo(x, y); }
  scrollBy(x, y) { if (typeof x === 'object' && x !== null) { y = x.top; x = x.left; } W.scrollTo(this, this.scrollLeft + (+x || 0), this.scrollTop + (+y || 0)); }
  get scrollTopMax() { return Math.max(0, this.scrollHeight - this.clientHeight); }
  setPointerCapture() {}
  releasePointerCapture() {}
  hasPointerCapture() { return false; }
  requestFullscreen() { return Promise.reject(domError('TypeError', 'Fullscreen is not supported.')); }
  requestPointerLock() {}
  animate(keyframes, options) { return new Animation(this, options); }
  getAnimations() { return []; }
  checkVisibility(options) {
    if (!W.isRendered(this)) return false;
    if (options && (options.checkVisibilityCSS || options.visibilityProperty) && getComputedStyle(this).visibility !== 'visible') return false;
    return true;
  }
  computedStyleMap() { const cs = getComputedStyle(this); return { get: (p) => ({ toString: () => cs.getPropertyValue(p) }) }; }
  get assignedSlot() { return null; }
  get [Symbol.toStringTag]() { return 'Element'; }
}
mixin(Element.prototype, parentNodeMethods);
mixin(Element.prototype, childNodeMethods);

class Animation extends C.EventTarget {
  constructor(target, options) {
    super();
    this.effect = { target, getTiming: () => ({ duration: (options && (typeof options === 'number' ? options : options.duration)) || 0 }) };
    this.playState = 'finished';
    this.currentTime = 0;
    this.playbackRate = 1;
    this.id = '';
    this.finished = Promise.resolve(this);
    this.ready = Promise.resolve(this);
    this.onfinish = null; this.oncancel = null;
  }
  play() { this.playState = 'finished'; } pause() {} cancel() { this.playState = 'idle'; } finish() { this.playState = 'finished'; } reverse() {} persist() {} commitStyles() {} updatePlaybackRate() {}
}

function reflectString(proto, prop, attr, defaultValue) {
  attr = attr || prop.toLowerCase();
  Object.defineProperty(proto, prop, {
    get() { const v = this.getAttribute(attr); return v === null ? (defaultValue === undefined ? '' : defaultValue) : v; },
    set(v) { this.setAttribute(attr, String(v)); },
    configurable: true, enumerable: true,
  });
}
function reflectBool(proto, prop, attr) {
  attr = attr || prop.toLowerCase();
  Object.defineProperty(proto, prop, {
    get() { return this.hasAttribute(attr); },
    set(v) { if (v) this.setAttribute(attr, ''); else this.removeAttribute(attr); },
    configurable: true, enumerable: true,
  });
}
function reflectInt(proto, prop, attr, defaultValue, nonNegative) {
  attr = attr || prop.toLowerCase();
  Object.defineProperty(proto, prop, {
    get() { const v = this.getAttribute(attr); const n = v === null ? NaN : parseInt(v, 10); return Number.isNaN(n) || (nonNegative && n < 0) ? defaultValue : n; },
    set(v) { v = Math.trunc(+v) || 0; if (nonNegative && v < 0) throw domError('IndexSizeError', 'The value provided (' + v + ') is negative.'); this.setAttribute(attr, String(v)); },
    configurable: true, enumerable: true,
  });
}
function reflectUrl(proto, prop, attr) {
  attr = attr || prop.toLowerCase();
  Object.defineProperty(proto, prop, {
    get() { const v = this.getAttribute(attr); return v === null ? '' : W.resolveUrl(v, document.baseURI); },
    set(v) { this.setAttribute(attr, String(v)); },
    configurable: true, enumerable: true,
  });
}

// ---------------------------------------------------------------- HTMLElement

class HTMLElement extends Element {
  constructor() {
    // `new MyElement()` for a defined custom element, or an upgrade in progress.
    const ctor = new.target;
    const def = customRegistry.byCtor.get(ctor);
    if (!def) throw typeError('Illegal constructor');
    if (upgrading.length) {
      const el = upgrading.pop();
      W.setProto(el, ctor.prototype);
      return el;
    }
    const el = W.createElement(def.extends || def.name, '');
    if (def.extends) W.setAttribute(el, 'is', def.name);
    W.setProto(el, ctor.prototype);
    define(el, '%upgraded', true);
    return el;
  }
  get style() {
    let s = this['%style'];
    if (!s) { s = W.inlineStyle(this); define(this, '%style', s); }
    return s;
  }
  get dataset() {
    let d = this['%dataset'];
    if (!d) { d = W.elementPart(this, 'dataset', 'DOMStringMap'); define(this, '%dataset', d); }
    return d;
  }
  get innerText() { return W.innerText(this); }
  set innerText(v) {
    const parts = String(v).split('\n');
    const frag = W.createFragment();
    parts.forEach((p, i) => { if (i > 0) W.insertBefore(frag, W.createElement('br', ''), null); if (p) W.insertBefore(frag, W.createText(p), null); });
    this.textContent = '';
    if (frag.firstChild) W.insertBefore(this, frag, null);
  }
  get outerText() { return this.innerText; }
  set outerText(v) { this.innerText = v; }
  get hidden() { return this.hasAttribute('hidden'); }
  set hidden(v) { if (v) this.setAttribute('hidden', ''); else this.removeAttribute('hidden'); }
  get tabIndex() {
    const v = this.getAttribute('tabindex');
    if (v !== null && !Number.isNaN(parseInt(v, 10))) return parseInt(v, 10);
    return ['a', 'area', 'button', 'input', 'select', 'textarea', 'summary', 'iframe'].includes(this.localName) ? 0 : -1;
  }
  set tabIndex(v) { this.setAttribute('tabindex', String(Math.trunc(+v) || 0)); }
  get isContentEditable() { const c = this.getAttribute('contenteditable'); return c !== null && c !== 'false'; }
  get contentEditable() { const c = this.getAttribute('contenteditable'); return c === null ? 'inherit' : c === '' ? 'true' : c; }
  set contentEditable(v) { this.setAttribute('contenteditable', String(v)); }
  get offsetParent() { return W.docPart('html') && super.offsetParent; }
  focus(options) {
    if (!W.isFocusable(this)) return;
    const old = W.docPart('focused');
    if (old === this) return;
    W.setFocused(this, !!(options && options.focusVisible));
    hooks.focusChange(old, this);
    if (!(options && options.preventScroll)) W.scrollIntoView(this, true, 'nearest');
  }
  blur() {
    if (W.docPart('focused') !== this) return;
    W.setFocused(null, false);
    hooks.focusChange(this, null);
  }
  click() {
    if (W.isDisabled(this) && ['button', 'input', 'select', 'textarea', 'fieldset'].includes(this.localName)) return;
    if (this['%clicking']) return;
    define(this, '%clicking', true);
    try { hooks.syntheticClick(this); } finally { define(this, '%clicking', false); }
  }
  get draggable() { return this.getAttribute('draggable') === 'true' || (this.localName === 'img' && this.getAttribute('draggable') !== 'false'); }
  set draggable(v) { this.setAttribute('draggable', v ? 'true' : 'false'); }
  get inert() { return this.hasAttribute('inert'); }
  set inert(v) { if (v) this.setAttribute('inert', ''); else this.removeAttribute('inert'); }
  get popover() { return this.getAttribute('popover'); }
  set popover(v) { if (v === null) this.removeAttribute('popover'); else this.setAttribute('popover', v); }
  showPopover() { this.removeAttribute('hidden'); }
  hidePopover() {}
  togglePopover() {}
  attachInternals() { return new ElementInternals(this); }
  get [Symbol.toStringTag]() { return 'HTMLElement'; }
}
for (const p of ['title', 'lang', 'dir', 'accessKey', 'autocapitalize', 'nonce', 'enterKeyHint', 'inputMode']) reflectString(HTMLElement.prototype, p);
reflectBool(HTMLElement.prototype, 'spellcheck');
reflectBool(HTMLElement.prototype, 'translate');
for (const t of globalEventTypes) defineHandlerAttr(HTMLElement.prototype, t);
// SVG elements and plain Elements handle events too.
for (const t of ['click', 'load', 'error', 'mousedown', 'mouseup', 'mouseover', 'mouseout', 'mousemove', 'keydown', 'keyup', 'focus', 'blur', 'input', 'change', 'scroll', 'wheel', 'pointerdown', 'pointerup', 'pointermove', 'touchstart', 'touchend', 'touchmove']) if (!Object.getOwnPropertyDescriptor(Element.prototype, 'on' + t)) defineHandlerAttr(Element.prototype, t);

class ElementInternals {
  constructor(el) { this._el = el; this._value = null; this.validity = { valid: true }; this.validationMessage = ''; }
  get form() { return this._el.form || null; }
  get shadowRoot() { return this._el.shadowRoot; }
  setFormValue(v) { this._value = v; }
  setValidity() {} checkValidity() { return true; } reportValidity() { return true; }
  get willValidate() { return true; }
  get labels() { return W.collection(this._el, 'labels', '', 'NodeList'); }
  get states() { return new Set(); }
}
class HTMLUnknownElement extends HTMLElement {}

// ---------------------------------------------------------------- SVG

class SVGAnimatedString { constructor(el, attr) { this._el = el; this._attr = attr; } get baseVal() { return this._el.getAttribute(this._attr) || ''; } set baseVal(v) { this._el.setAttribute(this._attr, v); } get animVal() { return this.baseVal; } }
class SVGAnimatedLength { constructor(el, attr) { this._el = el; this._attr = attr; } get baseVal() { const v = parseFloat(this._el.getAttribute(this._attr)) || 0; return { value: v, valueAsString: String(v), unitType: 1, valueInSpecifiedUnits: v }; } get animVal() { return this.baseVal; } }
class SVGElement extends Element {
  constructor() { super(); throw typeError('Illegal constructor'); }
  get className() { let c = this['%svgClass']; if (!c) { c = new SVGAnimatedString(this, 'class'); define(this, '%svgClass', c); } return c; }
  set className(v) { this.setAttribute('class', String(v)); }
  get style() { let s = this['%style']; if (!s) { s = W.inlineStyle(this); define(this, '%style', s); } return s; }
  get dataset() { let d = this['%dataset']; if (!d) { d = W.elementPart(this, 'dataset', 'DOMStringMap'); define(this, '%dataset', d); } return d; }
  get ownerSVGElement() { let p = this.parentNode; while (p && p.nodeType === 1) { if (p.localName === 'svg' && p.namespaceURI === 'http://www.w3.org/2000/svg') return p; p = p.parentNode; } return null; }
  get viewportElement() { return this.ownerSVGElement; }
  focus() { HTMLElement.prototype.focus.call(this); }
  blur() { HTMLElement.prototype.blur.call(this); }
  get tabIndex() { const v = this.getAttribute('tabindex'); return v === null ? -1 : parseInt(v, 10) || 0; }
  set tabIndex(v) { this.setAttribute('tabindex', String(v | 0)); }
  get [Symbol.toStringTag]() { return 'SVGElement'; }
}
for (const t of globalEventTypes) defineHandlerAttr(SVGElement.prototype, t);
class SVGGraphicsElement extends SVGElement {
  getBBox() { const r = W.boundingRect(this); return new DOMRect(0, 0, r[2], r[3]); }
  getCTM() { return null; } getScreenCTM() { return null; }
}
class SVGSVGElement extends SVGGraphicsElement {
  get width() { return new SVGAnimatedLength(this, 'width'); }
  get height() { return new SVGAnimatedLength(this, 'height'); }
  createSVGPoint() { return { x: 0, y: 0, matrixTransform() { return this; } }; }
  createSVGMatrix() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0, inverse() { return this; }, translate() { return this; }, scale() { return this; } }; }
  getElementById(id) { return W.getElementById(this, id); }
}
class SVGGeometryElement extends SVGGraphicsElement { getTotalLength() { return 0; } getPointAtLength() { return { x: 0, y: 0 }; } }
class SVGPathElement extends SVGGeometryElement {}
class SVGUseElement extends SVGGraphicsElement { get href() { return new SVGAnimatedString(this, 'href'); } }

// ---------------------------------------------------------------- Document

class DocumentFragment extends Node {
  constructor() {
    const f = W.createFragment();
    W.setProto(f, new.target.prototype);
    return f;
  }
  getElementById(id) { return W.getElementById(this, String(id)); }
  get [Symbol.toStringTag]() { return 'DocumentFragment'; }
}
mixin(DocumentFragment.prototype, parentNodeMethods);
class ShadowRoot extends DocumentFragment {
  constructor() { super(); throw typeError('Illegal constructor'); }
  get mode() { return this._mode; }
  get host() { return this._host; }
  get delegatesFocus() { return this._delegatesFocus; }
  get innerHTML() { return W.innerHTML(this); }
  set innerHTML(v) { W.setInnerHTML(this, v); }
  get activeElement() { const a = document.activeElement; return a && this.contains(a) ? a : null; }
  get styleSheets() { return C.makeStyleSheetList([]); }
  get adoptedStyleSheets() { return this._adopted || (this._adopted = []); }
  set adoptedStyleSheets(v) { this._adopted = Array.from(v); }
  getSelection() { return selection; }
  elementFromPoint(x, y) { return document.elementFromPoint(x, y); }
  get [Symbol.toStringTag]() { return 'ShadowRoot'; }
}
// A shadow root's content renders as light content: inserting into the root
// inserts into the host. Fragment insertion moves children, so the root itself
// forwards its tree operations to the host.
for (const m of ['appendChild', 'insertBefore', 'removeChild', 'replaceChild']) {
  define(ShadowRoot.prototype, m, function (...args) { return Node.prototype[m].apply(this._host, args); });
}
for (const m of ['append', 'prepend', 'replaceChildren', 'querySelector', 'querySelectorAll', 'getElementsByTagName', 'getElementsByClassName']) {
  define(ShadowRoot.prototype, m, function (...args) { return parentNodeMethods[m].apply(this._host, args); });
}
Object.defineProperty(ShadowRoot.prototype, 'children', { get() { return this._host.children; }, configurable: true });
Object.defineProperty(ShadowRoot.prototype, 'childNodes', { get() { return this._host.childNodes; }, configurable: true });
Object.defineProperty(ShadowRoot.prototype, 'firstChild', { get() { return this._host.firstChild; }, configurable: true });
Object.defineProperty(ShadowRoot.prototype, 'lastChild', { get() { return this._host.lastChild; }, configurable: true });
Object.defineProperty(ShadowRoot.prototype, 'textContent', { get() { return this._host.textContent; }, set(v) { this._host.textContent = v; }, configurable: true });
Object.defineProperty(ShadowRoot.prototype, 'innerHTML', { get() { return W.innerHTML(this._host); }, set(v) { W.setInnerHTML(this._host, v); }, configurable: true });
define(ShadowRoot.prototype, 'getElementById', function (id) { return W.getElementById(this._host, String(id)); });

class DOMImplementation {
  createHTMLDocument(title) {
    const d = W.parseDocument('<!DOCTYPE html><html><head>' + (title !== undefined ? '<title>' + String(title).replace(/</g, '&lt;') + '</title>' : '') + '</head><body></body></html>');
    return makeDetachedDocument(d);
  }
  createDocument(ns, name) { const d = W.parseDocument(''); return makeDetachedDocument(d); }
  createDocumentType(name, pub, sys) { return W.createDoctype(String(name), String(pub || ''), String(sys || '')); }
  hasFeature() { return true; }
}

// A detached document (DOMParser, createHTMLDocument) is a fragment holding the
// parsed tree, given the document's members.
function makeDetachedDocument(frag) {
  const html = Array.from(frag.childNodes).find((n) => n.nodeType === 1) || null;
  const find = (tag) => (html ? Array.from(html.childNodes).find((n) => n.nodeType === 1 && n.localName === tag) || null : null);
  define(frag, 'documentElement', html);
  define(frag, 'head', find('head'));
  define(frag, 'body', find('body'));
  define(frag, 'doctype', Array.from(frag.childNodes).find((n) => n.nodeType === 10) || null);
  define(frag, 'nodeType', 9);
  define(frag, 'nodeName', '#document');
  define(frag, 'getElementById', (id) => W.getElementById(frag, String(id)));
  define(frag, 'createElement', (t) => document.createElement(t));
  define(frag, 'createElementNS', (ns, t) => document.createElementNS(ns, t));
  define(frag, 'createTextNode', (t) => document.createTextNode(t));
  define(frag, 'createDocumentFragment', () => document.createDocumentFragment());
  define(frag, 'createComment', (t) => document.createComment(t));
  define(frag, 'importNode', (n, deep) => n.cloneNode(deep));
  define(frag, 'adoptNode', (n) => n);
  define(frag, 'implementation', new DOMImplementation());
  define(frag, 'defaultView', null);
  define(frag, 'readyState', 'complete');
  define(frag, 'URL', 'about:blank');
  define(frag, 'documentURI', 'about:blank');
  define(frag, 'contentType', 'text/html');
  define(frag, 'ownerDocument', null);
  Object.defineProperty(frag, 'title', { get() { const t = frag.querySelector('title'); return t ? t.textContent.trim() : ''; }, set(v) { let t = frag.querySelector('title'); if (!t) { t = document.createElement('title'); (frag.head || frag).appendChild(t); } t.textContent = v; }, configurable: true });
  Object.defineProperty(frag, 'forms', { get() { return W.collection(frag, 'forms', '', 'HTMLCollection'); }, configurable: true });
  Object.defineProperty(frag, 'images', { get() { return W.collection(frag, 'images', '', 'HTMLCollection'); }, configurable: true });
  Object.defineProperty(frag, 'links', { get() { return W.collection(frag, 'links', '', 'HTMLCollection'); }, configurable: true });
  Object.defineProperty(frag, 'scripts', { get() { return W.collection(frag, 'scripts', '', 'HTMLCollection'); }, configurable: true });
  Object.defineProperty(frag, 'styleSheets', { get() { return C.makeStyleSheetList([]); }, configurable: true });
  return frag;
}

class Document extends Node {
  constructor() {
    const d = W.parseDocument('');
    return makeDetachedDocument(d);
  }
  get documentElement() { return W.docPart('html'); }
  get head() { return W.docPart('head'); }
  get body() { return W.docPart('body'); }
  set body(v) { const old = this.body; if (old) W.replaceChild(this.documentElement, v, old); else W.insertBefore(this.documentElement, v, null); }
  get doctype() { return W.docPart('doctype'); }
  get title() { return W.docInfo('title'); }
  set title(v) {
    let t = this.querySelector('title');
    if (!t) { t = W.createElement('title', ''); const h = this.head; if (!h) return; W.insertBefore(h, t, null); }
    t.textContent = String(v);
  }
  get nodeValue() { return null; }
  get URL() { return W.docInfo('url'); }
  get documentURI() { return W.docInfo('url'); }
  get baseURI() { const b = this.querySelector('base[href]'); return b ? W.resolveUrl(b.getAttribute('href'), W.docInfo('url')) : W.docInfo('url'); }
  get location() { return globalThis.location; }
  set location(v) { globalThis.location.href = v; }
  get referrer() { return W.docInfo('referrer'); }
  get domain() { return globalThis.location.hostname; }
  set domain(v) {}
  get readyState() { return W.docInfo('readyState'); }
  get compatMode() { return W.docInfo('compatMode'); }
  get characterSet() { return 'UTF-8'; }
  get charset() { return 'UTF-8'; }
  get inputEncoding() { return 'UTF-8'; }
  get contentType() { return 'text/html'; }
  get designMode() { return 'off'; }
  set designMode(v) {}
  get dir() { const h = this.documentElement; return h ? h.dir : ''; }
  set dir(v) { const h = this.documentElement; if (h) h.dir = v; }
  get hidden() { return W.docInfo('hidden'); }
  get visibilityState() { return W.docInfo('hidden') ? 'hidden' : 'visible'; }
  get defaultView() { return globalThis; }
  get implementation() { let i = this['%impl']; if (!i) { i = new DOMImplementation(); define(this, '%impl', i); } return i; }
  get activeElement() { return W.docPart('focused') || this.body; }
  get scrollingElement() { return this.compatMode === 'BackCompat' ? this.body : this.documentElement; }
  get currentScript() { return W.docPart('currentScript'); }
  get cookie() { return W.docInfo('cookie'); }
  set cookie(v) { W.setCookie(String(v)); }
  get fullscreenElement() { return null; }
  get fullscreenEnabled() { return false; }
  get pointerLockElement() { return null; }
  get fonts() { let f = this['%fonts']; if (!f) { f = new FontFaceSet(); define(this, '%fonts', f); } return f; }
  get timeline() { return { currentTime: performance.now() }; }
  get styleSheets() { return C.makeStyleSheetList(W.docSheets()); }
  get adoptedStyleSheets() { return C.adoptedSheets(); }
  set adoptedStyleSheets(v) { C.setAdoptedSheets(v); }
  get forms() { return W.collection(this, 'forms', '', 'HTMLCollection'); }
  get images() { return W.collection(this, 'images', '', 'HTMLCollection'); }
  get links() { return W.collection(this, 'links', '', 'HTMLCollection'); }
  get scripts() { return W.collection(this, 'scripts', '', 'HTMLCollection'); }
  get embeds() { return W.collection(this, 'embeds', '', 'HTMLCollection'); }
  get plugins() { return this.embeds; }
  get anchors() { return W.collection(this, 'anchors', '', 'HTMLCollection'); }
  get all() { return W.collection(this, 'tag', '*', 'HTMLCollection'); }
  hasFocus() { return !W.docInfo('hidden'); }
  getElementById(id) { return W.getElementById(this, String(id)); }
  getElementsByName(name) { return W.collection(this, 'name', String(name), 'NodeList'); }
  createElement(tag, options) {
    tag = String(tag);
    const el = W.createElement(tag, '');
    const is = options && typeof options === 'object' ? options.is : undefined;
    if (is) { W.setAttribute(el, 'is', is); }
    maybeUpgrade(el, is ? String(is) : tag.toLowerCase());
    return el;
  }
  createElementNS(ns, tag, options) {
    if (ns === 'http://www.w3.org/1999/xhtml' || ns === null || ns === '') return this.createElement(tag, options);
    const name = String(tag).includes(':') ? String(tag).split(':')[1] : String(tag);
    return W.createElement(name, String(ns));
  }
  createTextNode(data) { return W.createText(String(data)); }
  createComment(data) { return W.createComment(String(data)); }
  createCDATASection(data) { return W.createText(String(data)); }
  createProcessingInstruction(target, data) { return W.createComment('?' + target + ' ' + data + '?'); }
  createDocumentFragment() { return W.createFragment(); }
  createAttribute(name) { const a = new C.Attr(this.documentElement || this.body || W.createElement('div', ''), String(name).toLowerCase()); define(a, '_owner', null); define(a, '_detachedValue', ''); return a; }
  createAttributeNS(ns, name) { return this.createAttribute(name); }
  createRange() { const r = new Range(); r.setStart(this, 0); r.setEnd(this, 0); return r; }
  createTreeWalker(root, whatToShow, filter) { return new TreeWalker(root, whatToShow, filter); }
  createNodeIterator(root, whatToShow, filter) { return new NodeIterator(root, whatToShow, filter); }
  createEvent(kind) {
    const k = String(kind).toLowerCase();
    const ctor = { event: C.Event, events: C.Event, htmlevents: C.Event, customevent: C.CustomEvent, uievent: C.UIEvent, uievents: C.UIEvent, mouseevent: C.MouseEvent, mouseevents: C.MouseEvent, keyboardevent: C.KeyboardEvent, focusevent: C.FocusEvent, inputevent: C.InputEvent, touchevent: C.TouchEvent, messageevent: C.MessageEvent, hashchangeevent: C.HashChangeEvent, popstateevent: C.PopStateEvent, storageevent: C.StorageEvent, wheelevent: C.WheelEvent, dragevent: C.DragEvent, textevent: C.CompositionEvent, compositionevent: C.CompositionEvent }[k];
    if (!ctor) throw domError('NotSupportedError', "The provided event type ('" + kind + "') is invalid.");
    const ev = new ctor('');
    ev._type = '';
    return ev;
  }
  importNode(node, deep) { return node.cloneNode(!!deep); }
  adoptNode(node) { if (node.parentNode) W.remove(node); return node; }
  getSelection() { return selection; }
  elementFromPoint(x, y) { return W.elementFromPoint(+x || 0, +y || 0); }
  elementsFromPoint(x, y) { return W.elementsFromPoint(+x || 0, +y || 0); }
  caretPositionFromPoint() { return null; }
  caretRangeFromPoint(x, y) { const el = this.elementFromPoint(x, y); if (!el) return null; const r = new Range(); r.selectNodeContents(el); r.collapse(true); return r; }
  execCommand() { return false; }
  queryCommandSupported() { return false; }
  queryCommandEnabled() { return false; }
  queryCommandState() { return false; }
  queryCommandValue() { return ''; }
  open() { if (W.docInfo('readyState') !== 'loading') { W.docReplace(''); define(this, '%writeOpen', true); } return this; }
  close() { define(this, '%writeOpen', false); }
  write(...args) { const s = args.map(String).join(''); if (W.docWrite(s)) return; if (!this['%writeOpen']) { define(this, '%writeOpen', true); W.docReplace(s); } else { const target = this.body || this.documentElement; if (target) W.insertBefore(target, W.parseFragment(target, s), null); } }
  writeln(...args) { this.write(args.map(String).join('') + '\n'); }
  exitFullscreen() { return Promise.resolve(); }
  exitPointerLock() {}
  requestStorageAccess() { return Promise.resolve(); }
  hasStorageAccess() { return Promise.resolve(true); }
  startViewTransition(cb) { if (typeof cb === 'function') cb(); return { finished: Promise.resolve(), ready: Promise.resolve(), updateCallbackDone: Promise.resolve(), skipTransition() {} }; }
  get [Symbol.toStringTag]() { return 'HTMLDocument'; }
}
mixin(Document.prototype, parentNodeMethods);
for (const t of globalEventTypes.concat(documentEventTypes)) defineHandlerAttr(Document.prototype, t);
class HTMLDocument extends Document {}
class XMLDocument extends Document {}

class FontFaceSet extends C.EventTarget {
  constructor() { super(); this.ready = Promise.resolve(this); this.status = 'loaded'; this.onloading = null; this.onloadingdone = null; this.onloadingerror = null; }
  load() { return Promise.resolve([]); } check() { return true; } add() { return this; } delete() { return false; } clear() {} has() { return false; } get size() { return 0; }
  forEach() {} values() { return [].values(); } keys() { return [].values(); } entries() { return [].entries(); } [Symbol.iterator]() { return [].values(); }
}
class FontFace {
  constructor(family, source, descriptors) { this.family = family; this.source = source; Object.assign(this, descriptors || {}); this.status = 'loaded'; this.loaded = Promise.resolve(this); }
  load() { return this.loaded; }
}

class DOMParser {
  parseFromString(html, type) {
    const d = W.parseDocument(String(html));
    const doc = makeDetachedDocument(d);
    if (type && String(type).includes('xml') && !String(type).includes('html')) define(doc, 'contentType', String(type));
    return doc;
  }
}
class XMLSerializer { serializeToString(node) { return node.nodeType === 9 || node.nodeType === 11 ? W.innerHTML(node) : W.outerHTML(node); } }

// ---------------------------------------------------------------- custom elements

const customRegistry = { byName: new Map(), byCtor: new Map(), whenDefined: new Map() };
const upgrading = [];
class CustomElementRegistry {
  define(name, ctor, options) {
    name = String(name);
    if (!/^[a-z][.0-9_a-z·À-퟿豈-﷏ﷰ-�]*-[.0-9_a-z·À-퟿豈-﷏ﷰ-�]*$/.test(name) || ['annotation-xml', 'color-profile', 'font-face', 'font-face-src', 'font-face-uri', 'font-face-format', 'font-face-name', 'missing-glyph'].includes(name)) throw domError('SyntaxError', "Failed to execute 'define' on 'CustomElementRegistry': \"" + name + "\" is not a valid custom element name");
    if (typeof ctor !== 'function') throw typeError("Failed to execute 'define' on 'CustomElementRegistry': parameter 2 is not of type 'Function'.");
    if (customRegistry.byName.has(name)) throw domError('NotSupportedError', "Failed to execute 'define' on 'CustomElementRegistry': the name \"" + name + "\" has already been used with this registry");
    if (customRegistry.byCtor.has(ctor)) throw domError('NotSupportedError', "Failed to execute 'define' on 'CustomElementRegistry': this constructor has already been used with this registry");
    const observed = Array.from(ctor.observedAttributes || []).map(String);
    const def = { name, ctor, extends: options && options.extends ? String(options.extends) : null, observed };
    customRegistry.byName.set(name, def);
    customRegistry.byCtor.set(ctor, def);
    const candidates = W.defineCustom(name);
    for (const el of candidates) upgradeElement(el, def);
    const w = customRegistry.whenDefined.get(name);
    if (w) { w.resolve(ctor); customRegistry.whenDefined.delete(name); }
  }
  get(name) { const d = customRegistry.byName.get(String(name)); return d ? d.ctor : undefined; }
  getName(ctor) { const d = customRegistry.byCtor.get(ctor); return d ? d.name : null; }
  whenDefined(name) {
    name = String(name);
    const d = customRegistry.byName.get(name);
    if (d) return Promise.resolve(d.ctor);
    let w = customRegistry.whenDefined.get(name);
    if (!w) { let resolve; const promise = new Promise((r) => { resolve = r; }); w = { promise, resolve }; customRegistry.whenDefined.set(name, w); }
    return w.promise;
  }
  upgrade(root) { for (const el of W.subtreeElements(root)) maybeUpgrade(el, el.getAttribute('is') || el.localName); }
}
function maybeUpgrade(el, name) {
  const def = customRegistry.byName.get(name);
  if (def && !el['%upgraded']) upgradeElement(el, def);
}
function upgradeElement(el, def) {
  if (el['%upgraded']) return;
  define(el, '%upgraded', true);
  upgrading.push(el);
  try {
    const r = new def.ctor();
    if (r !== el) { upgrading.length = 0; }
  } catch (e) {
    upgrading.length = 0;
    C.reportError(e);
    return;
  }
  for (const attr of def.observed) {
    const v = W.getAttributeRaw(el, attr);
    if (v !== null && typeof el.attributeChangedCallback === 'function') { try { el.attributeChangedCallback(attr, null, v); } catch (e) { C.reportError(e); } }
  }
  if (el.isConnected && typeof el.connectedCallback === 'function') { define(el, '%connected', true); try { el.connectedCallback(); } catch (e) { C.reportError(e); } }
}
const customElements = new CustomElementRegistry();

// Insertion/removal reactions (from the natives, for subtrees with scripts,
// styles, images or custom elements).
hooks.inserted = (node) => {
  for (const el of W.subtreeElements(node)) {
    const tag = el.localName;
    if (tag === 'script') { try { W.runScript(el); } catch (e) { C.reportError(e); } continue; }
    if (tag === 'img') { C.imageInserted(el); continue; }
    if (tag === 'link' || tag === 'style') { C.styleInserted(el); continue; }
    if (tag === 'iframe') { queueTask(() => fireSimple(el, 'load', false, false)); continue; }
    const name = el.getAttribute('is') || tag;
    const def = customRegistry.byName.get(name);
    if (def) {
      if (!el['%upgraded']) upgradeElement(el, def);
      else if (!el['%connected'] && typeof el.connectedCallback === 'function') { define(el, '%connected', true); try { el.connectedCallback(); } catch (e) { C.reportError(e); } }
    }
  }
};
hooks.removed = (node) => {
  for (const el of W.subtreeElements(node)) {
    if (el['%upgraded'] && el['%connected'] && typeof el.disconnectedCallback === 'function') { define(el, '%connected', false); try { el.disconnectedCallback(); } catch (e) { C.reportError(e); } }
  }
};
hooks.attribute = (el, name, oldValue, newValue) => {
  if (C.mutationObserving) C.attributeRecord(el, name, oldValue);
  if (el['%upgraded']) {
    const def = customRegistry.byName.get(el.getAttribute('is') || el.localName);
    if (def && def.observed.includes(name) && typeof el.attributeChangedCallback === 'function') { try { el.attributeChangedCallback(name, oldValue, newValue); } catch (e) { C.reportError(e); } }
  }
};
function queueTask(fn) { setTimeout(fn, 0); }

Object.assign(C, { Element, HTMLElement, HTMLUnknownElement, ElementInternals, Animation, SVGElement, SVGGraphicsElement, SVGSVGElement, SVGGeometryElement, SVGPathElement, SVGUseElement, SVGAnimatedString, SVGAnimatedLength, DocumentFragment, ShadowRoot, Document, HTMLDocument, XMLDocument, DOMImplementation, DOMParser, XMLSerializer, FontFaceSet, FontFace, CustomElementRegistry, customElements, customRegistry, maybeUpgrade, reflectString, reflectBool, reflectInt, reflectUrl, queueTask, makeDetachedDocument });
})();
