// A minimal DOM in plain JavaScript, just enough for the production builds of
// React 18 and Vue 3 in crates/web/engine/tests/vendor to mount the
// framework-parity task tracker and handle a click and a keystroke. It runs as
// ordinary script in both engines being compared (cw-jsvm and Node), so the
// DOM's own cost is interpreted JS on both sides. No layout, no CSS, no
// selectors beyond `#id`.
(function (g) {
  'use strict';
  const HTML_NS = 'http://www.w3.org/1999/xhtml';
  let clock = 0;

  class EventTarget {
    constructor() {
      this._listeners = null;
    }
    addEventListener(type, fn, opts) {
      if (!fn) return;
      const capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
      const once = !!(opts && typeof opts === 'object' && opts.once);
      if (!this._listeners) this._listeners = Object.create(null);
      const list = this._listeners[type] || (this._listeners[type] = []);
      for (const l of list) if (l.fn === fn && l.capture === capture) return;
      list.push({ fn, capture, once });
    }
    removeEventListener(type, fn, opts) {
      const capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
      const list = this._listeners && this._listeners[type];
      if (!list) return;
      const i = list.findIndex((l) => l.fn === fn && l.capture === capture);
      if (i >= 0) list.splice(i, 1);
    }
    _invoke(event, phase) {
      const list = this._listeners && this._listeners[event.type];
      if (!list) return;
      for (const l of list.slice()) {
        if (phase === 1 && !l.capture) continue;
        if (phase === 3 && l.capture) continue;
        if (l.once) this.removeEventListener(event.type, l.fn, l.capture);
        event.currentTarget = this;
        if (typeof l.fn === 'function') l.fn.call(this, event);
        else l.fn.handleEvent(event);
        if (event._stopImmediate) return;
      }
    }
    dispatchEvent(event) {
      event.target = this;
      event.isTrusted = false;
      const path = [];
      for (let n = this.parentNode; n; n = n.parentNode) path.push(n);
      if (this.nodeType === 9 || path.length && path[path.length - 1].nodeType === 9) path.push(g);
      event.eventPhase = 1;
      for (let i = path.length - 1; i >= 0 && !event._stop; i--) path[i]._invoke(event, 1);
      if (!event._stop) {
        event.eventPhase = 2;
        this._invoke(event, 2);
      }
      if (event.bubbles) {
        event.eventPhase = 3;
        for (let i = 0; i < path.length && !event._stop; i++) path[i]._invoke(event, 3);
      }
      event.eventPhase = 0;
      event.currentTarget = null;
      return !event.defaultPrevented;
    }
  }

  class Event {
    constructor(type, init) {
      init = init || {};
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.composed = !!init.composed;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.eventPhase = 0;
      this.timeStamp = ++clock;
      this.isTrusted = false;
      this._stop = false;
      this._stopImmediate = false;
    }
    preventDefault() {
      if (this.cancelable) this.defaultPrevented = true;
    }
    get returnValue() {
      return !this.defaultPrevented;
    }
    stopPropagation() {
      this._stop = true;
    }
    stopImmediatePropagation() {
      this._stop = true;
      this._stopImmediate = true;
    }
    get srcElement() {
      return this.target;
    }
    composedPath() {
      const p = [];
      for (let n = this.target; n; n = n.parentNode) p.push(n);
      return p;
    }
  }
  class UIEvent extends Event {
    constructor(type, init) {
      super(type, init);
      this.view = (init && init.view) || null;
      this.detail = (init && init.detail) || 0;
    }
  }
  class MouseEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.clientX = init.clientX || 0;
      this.clientY = init.clientY || 0;
      this.screenX = init.screenX || 0;
      this.screenY = init.screenY || 0;
      this.pageX = this.clientX;
      this.pageY = this.clientY;
      this.button = init.button || 0;
      this.buttons = init.buttons || 0;
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
      this.relatedTarget = init.relatedTarget || null;
    }
    getModifierState() {
      return false;
    }
  }
  class KeyboardEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.key = init.key || '';
      this.code = init.code || '';
      this.keyCode = init.keyCode || 0;
      this.charCode = init.charCode || 0;
      this.which = this.keyCode;
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
      this.repeat = !!init.repeat;
    }
    getModifierState() {
      return false;
    }
  }
  class InputEvent extends UIEvent {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.data = init.data === undefined ? null : init.data;
      this.inputType = init.inputType || '';
      this.isComposing = false;
    }
  }
  class FocusEvent extends UIEvent {}

  class Node extends EventTarget {
    constructor(type, name, doc) {
      super();
      this.nodeType = type;
      this.nodeName = name;
      this.ownerDocument = doc;
      this.parentNode = null;
      this.childNodes = [];
    }
    get parentElement() {
      const p = this.parentNode;
      return p && p.nodeType === 1 ? p : null;
    }
    get firstChild() {
      return this.childNodes[0] || null;
    }
    get lastChild() {
      return this.childNodes[this.childNodes.length - 1] || null;
    }
    get nextSibling() {
      const p = this.parentNode;
      if (!p) return null;
      return p.childNodes[p.childNodes.indexOf(this) + 1] || null;
    }
    get previousSibling() {
      const p = this.parentNode;
      if (!p) return null;
      return p.childNodes[p.childNodes.indexOf(this) - 1] || null;
    }
    hasChildNodes() {
      return this.childNodes.length > 0;
    }
    contains(n) {
      for (; n; n = n.parentNode) if (n === this) return true;
      return false;
    }
    _detach(n) {
      if (n.parentNode) {
        const s = n.parentNode.childNodes;
        s.splice(s.indexOf(n), 1);
        n.parentNode = null;
      }
    }
    appendChild(n) {
      return this.insertBefore(n, null);
    }
    insertBefore(n, ref) {
      if (n.nodeType === 11) {
        for (const c of n.childNodes.slice()) this.insertBefore(c, ref);
        return n;
      }
      this._detach(n);
      const i = ref ? this.childNodes.indexOf(ref) : -1;
      if (i < 0) this.childNodes.push(n);
      else this.childNodes.splice(i, 0, n);
      n.parentNode = this;
      return n;
    }
    removeChild(n) {
      if (n.parentNode !== this) throw new Error('NotFoundError: not a child');
      this._detach(n);
      return n;
    }
    replaceChild(n, old) {
      this.insertBefore(n, old);
      this.removeChild(old);
      return old;
    }
    remove() {
      if (this.parentNode) this.parentNode.removeChild(this);
    }
    get textContent() {
      if (this.nodeType === 3 || this.nodeType === 8) return this.data;
      let s = '';
      for (const c of this.childNodes) if (c.nodeType !== 8) s += c.textContent;
      return s;
    }
    set textContent(v) {
      if (this.nodeType === 3 || this.nodeType === 8) {
        this.data = String(v);
        return;
      }
      for (const c of this.childNodes) c.parentNode = null;
      this.childNodes = [];
      const s = v == null ? '' : String(v);
      if (s !== '') this.appendChild(this.ownerDocument.createTextNode(s));
    }
  }

  class CharacterData extends Node {
    constructor(type, name, doc, data) {
      super(type, name, doc);
      this.data = data;
    }
    get nodeValue() {
      return this.data;
    }
    set nodeValue(v) {
      this.data = String(v);
    }
    get length() {
      return this.data.length;
    }
  }
  class Text extends CharacterData {
    constructor(doc, data) {
      super(3, '#text', doc, data);
    }
  }
  class Comment extends CharacterData {
    constructor(doc, data) {
      super(8, '#comment', doc, data);
    }
  }

  function styleName(p) {
    return p.startsWith('--') ? p : p.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
  }
  class CSSStyleDeclaration {
    setProperty(p, v) {
      this[styleName(p)] = v == null ? '' : String(v);
    }
    removeProperty(p) {
      const k = styleName(p);
      const old = this[k] || '';
      delete this[k];
      return old;
    }
    getPropertyValue(p) {
      return this[styleName(p)] || '';
    }
    get cssText() {
      return Object.keys(this)
        .filter((k) => this[k] !== '')
        .map((k) => k.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase()) + ': ' + this[k])
        .join('; ');
    }
    set cssText(v) {
      for (const k of Object.keys(this)) delete this[k];
      for (const d of String(v).split(';')) {
        const i = d.indexOf(':');
        if (i > 0) this.setProperty(d.slice(0, i).trim(), d.slice(i + 1).trim());
      }
    }
  }

  class Element extends Node {
    constructor(doc, tag, ns) {
      super(1, tag.toUpperCase(), doc);
      this.tagName = this.nodeName;
      this.localName = tag.toLowerCase();
      this.namespaceURI = ns || HTML_NS;
      this._attrs = new Map();
      this.style = new CSSStyleDeclaration();
    }
    get id() {
      return this.getAttribute('id') || '';
    }
    set id(v) {
      this.setAttribute('id', v);
    }
    get className() {
      return this.getAttribute('class') || '';
    }
    set className(v) {
      this.setAttribute('class', v);
    }
    get classList() {
      const el = this;
      const list = () => el.className.split(/\s+/).filter(Boolean);
      return {
        add: (...c) => (el.className = [...new Set([...list(), ...c])].join(' ')),
        remove: (...c) => (el.className = list().filter((x) => !c.includes(x)).join(' ')),
        contains: (c) => list().includes(c),
        toggle: (c, f) => {
          const has = list().includes(c);
          const want = f === undefined ? !has : !!f;
          if (want && !has) el.className = [...list(), c].join(' ');
          if (!want && has) el.className = list().filter((x) => x !== c).join(' ');
          return want;
        },
      };
    }
    setAttribute(n, v) {
      n = String(n).toLowerCase();
      this._attrs.set(n, String(v));
      if (n === 'style') this.style.cssText = String(v);
    }
    setAttributeNS(ns, n, v) {
      this.setAttribute(n, v);
    }
    getAttribute(n) {
      const v = this._attrs.get(String(n).toLowerCase());
      return v === undefined ? null : v;
    }
    hasAttribute(n) {
      return this._attrs.has(String(n).toLowerCase());
    }
    removeAttribute(n) {
      this._attrs.delete(String(n).toLowerCase());
    }
    removeAttributeNS(ns, n) {
      this.removeAttribute(n);
    }
    get attributes() {
      return [...this._attrs].map(([name, value]) => ({ name, value }));
    }
    get children() {
      return this.childNodes.filter((c) => c.nodeType === 1);
    }
    get innerHTML() {
      return '';
    }
    set innerHTML(v) {
      if (v !== '') throw new Error('dom-shim: innerHTML only clears');
      this.textContent = '';
    }
    focus() {
      this.ownerDocument.activeElement = this;
    }
    blur() {
      if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = this.ownerDocument.body;
    }
    getBoundingClientRect() {
      return { x: 0, y: 0, left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 };
    }
    querySelector(sel) {
      return query(this, sel);
    }
  }

  class HTMLElement extends Element {}
  class HTMLInputElement extends HTMLElement {
    constructor(doc, tag) {
      super(doc, tag);
      this._value = null;
      this._checked = false;
    }
    get value() {
      return this._value === null ? this.getAttribute('value') || '' : this._value;
    }
    set value(v) {
      this._value = v == null ? '' : String(v);
    }
    get defaultValue() {
      return this.getAttribute('value') || '';
    }
    set defaultValue(v) {
      this.setAttribute('value', v);
    }
    get checked() {
      return this._checked;
    }
    set checked(v) {
      this._checked = !!v;
    }
    get type() {
      return this.getAttribute('type') || 'text';
    }
    set type(v) {
      this.setAttribute('type', v);
    }
  }
  class HTMLFormElement extends HTMLElement {}
  class HTMLIFrameElement extends HTMLElement {}
  class SVGElement extends Element {}
  class MathMLElement extends Element {}
  class HTMLButtonElement extends HTMLElement {
    get type() {
      return this.getAttribute('type') || 'submit';
    }
    set type(v) {
      this.setAttribute('type', v);
    }
  }

  function query(root, sel) {
    const m = /^#([\w-]+)$/.exec(sel);
    if (!m) throw new Error('dom-shim: only #id selectors: ' + sel);
    const stack = [...root.childNodes];
    while (stack.length) {
      const n = stack.shift();
      if (n.nodeType === 1 && n.getAttribute('id') === m[1]) return n;
      stack.unshift(...n.childNodes);
    }
    return null;
  }

  class DocumentFragment extends Node {
    constructor(doc) {
      super(11, '#document-fragment', doc);
    }
  }

  class Document extends Node {
    constructor() {
      super(9, '#document', null);
      this.documentElement = this.createElement('html');
      this.appendChild(this.documentElement);
      this.head = this.createElement('head');
      this.body = this.createElement('body');
      this.documentElement.appendChild(this.head);
      this.documentElement.appendChild(this.body);
      this.activeElement = this.body;
      this.readyState = 'complete';
    }
    createElement(tag) {
      const t = String(tag).toLowerCase();
      if (t === 'input' || t === 'textarea') return new HTMLInputElement(this, t);
      if (t === 'form') return new HTMLFormElement(this, t);
      if (t === 'button') return new HTMLButtonElement(this, t);
      return new HTMLElement(this, t);
    }
    createElementNS(ns, tag) {
      if (ns === 'http://www.w3.org/2000/svg') return new SVGElement(this, tag, ns);
      return new Element(this, tag, ns);
    }
    createTextNode(s) {
      return new Text(this, String(s));
    }
    createComment(s) {
      return new Comment(this, String(s));
    }
    createDocumentFragment() {
      return new DocumentFragment(this);
    }
    getElementById(id) {
      return query(this, '#' + id);
    }
    querySelector(sel) {
      return query(this, sel);
    }
    get defaultView() {
      return g;
    }
  }
  // `on<event>` handler slots, which feature detection looks for.
  for (const e of ['click', 'input', 'change', 'keydown', 'keyup', 'focus', 'blur', 'submit', 'mousedown', 'mouseup', 'mouseover', 'mouseout', 'pointerdown', 'pointerup', 'scroll', 'wheel', 'touchstart', 'animationend', 'transitionend', 'selectionchange']) {
    Document.prototype['on' + e] = null;
    HTMLElement.prototype['on' + e] = null;
  }

  const document = new Document();
  const app = document.createElement('div');
  app.setAttribute('id', 'app');
  document.body.appendChild(app);

  Object.setPrototypeOf(g, EventTarget.prototype);
  g._listeners = null;
  const globals = {
    window: g,
    self: g,
    document,
    Node,
    Element,
    HTMLElement,
    HTMLInputElement,
    HTMLFormElement,
    HTMLButtonElement,
    HTMLIFrameElement,
    SVGElement,
    MathMLElement,
    Text,
    Comment,
    Document,
    DocumentFragment,
    EventTarget,
    Event,
    UIEvent,
    MouseEvent,
    KeyboardEvent,
    InputEvent,
    FocusEvent,
    CSSStyleDeclaration,
    navigator: { userAgent: 'dom-shim', platform: 'Linux', language: 'en-US' },
    location: { href: 'https://example.test/', protocol: 'https:', host: 'example.test', hostname: 'example.test', pathname: '/', search: '', hash: '' },
    requestAnimationFrame: (f) => setTimeout(() => f(Date.now()), 16),
    cancelAnimationFrame: (id) => clearTimeout(id),
    getComputedStyle: (el) => el.style,
  };
  // Defined rather than assigned: Node has getter-only globals (`navigator`).
  for (const k of Object.keys(globals)) {
    Object.defineProperty(g, k, { value: globals[k], writable: true, configurable: true, enumerable: false });
  }
})(globalThis);
