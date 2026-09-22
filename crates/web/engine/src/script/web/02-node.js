// Prelude part 2: the node hierarchy, collections and ranges.
(function () {
'use strict';
const C = globalThis['%core'];
const { W, hooks, define, isNode, domError, typeError, SHADOW_HOST, EventTarget, defineHandlerAttr, globalEventTypes, fire, fireSimple } = C;

const nodeList = (arr) => W.staticList(arr, 'NodeList');
const toNodeOrText = (v) => (isNode(v) ? v : document.createTextNode(String(v)));
function convertNodes(nodes) {
  if (nodes.length === 1) return toNodeOrText(nodes[0]);
  const frag = document.createDocumentFragment();
  for (const n of nodes) W.insertBefore(frag, toNodeOrText(n), null);
  return frag;
}

// ---------------------------------------------------------------- Node

class Node extends EventTarget {
  constructor() { super(); throw typeError('Illegal constructor'); }
  get nodeValue() { return null; }
  set nodeValue(v) { /* elements ignore */ }
  get ownerDocument() { return this === document ? null : document; }
  get baseURI() { return document.baseURI; }
  get childNodes() {
    let c = this['%childNodes'];
    if (!c) { c = W.collection(this, 'childNodes', '', 'NodeList'); define(this, '%childNodes', c); }
    return c;
  }
  getRootNode(options) {
    let n = this;
    for (;;) {
      const p = n.parentNode;
      if (p) { n = p; continue; }
      const host = n[SHADOW_HOST];
      if (host && options && options.composed) { n = host; continue; }
      return n;
    }
  }
  appendChild(child) { return W.insertBefore(this, child, null); }
  insertBefore(child, ref) {
    if (ref === undefined && arguments.length < 2) throw typeError("Failed to execute 'insertBefore' on 'Node': 2 arguments required, but only 1 present.");
    return W.insertBefore(this, child, ref === undefined ? null : ref);
  }
  removeChild(child) { return W.removeChild(this, child); }
  replaceChild(node, child) { return W.replaceChild(this, node, child); }
  cloneNode(deep) {
    const c = W.cloneNode(this, !!deep);
    if (!deep && this.nodeType === 11 && this[SHADOW_HOST]) { /* shadow roots do not clone */ }
    return c;
  }
  normalize() { W.normalize(this); }
  isSameNode(other) { return this === other; }
  isDefaultNamespace(ns) { return ns === 'http://www.w3.org/1999/xhtml'; }
  lookupNamespaceURI(prefix) { return prefix === null || prefix === '' ? 'http://www.w3.org/1999/xhtml' : prefix === 'svg' ? 'http://www.w3.org/2000/svg' : null; }
  lookupPrefix() { return null; }
  get [Symbol.toStringTag]() { return 'Node'; }
}
const nodeConsts = { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, ENTITY_REFERENCE_NODE: 5, ENTITY_NODE: 6, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11, NOTATION_NODE: 12, DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16, DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: 32 };
for (const k in nodeConsts) { Object.defineProperty(Node, k, { value: nodeConsts[k], enumerable: true }); Object.defineProperty(Node.prototype, k, { value: nodeConsts[k], enumerable: true }); }

// ParentNode / ChildNode mixins.
const parentNodeMethods = {
  get children() {
    let c = this['%children'];
    if (!c) { c = W.collection(this, 'children', '', 'HTMLCollection'); define(this, '%children', c); }
    return c;
  },
  append(...nodes) { if (nodes.length) W.insertBefore(this, convertNodes(nodes), null); },
  prepend(...nodes) { if (nodes.length) W.insertBefore(this, convertNodes(nodes), this.firstChild); },
  replaceChildren(...nodes) {
    let c;
    while ((c = this.firstChild)) W.remove(c);
    if (nodes.length) W.insertBefore(this, convertNodes(nodes), null);
  },
  querySelector(sel) { return W.querySelector(this, sel); },
  querySelectorAll(sel) { return nodeList(W.querySelectorAll(this, sel)); },
  getElementsByTagName(name) { return W.collection(this, 'tag', String(name), 'HTMLCollection'); },
  getElementsByTagNameNS(ns, name) { return W.collection(this, 'tag', String(name), 'HTMLCollection'); },
  getElementsByClassName(names) { return W.collection(this, 'class', String(names), 'HTMLCollection'); },
};
const childNodeMethods = {
  remove() { W.remove(this); },
  before(...nodes) { const p = this.parentNode; if (!p) return; let ref = this.previousSibling; while (ref && nodes.includes(ref)) ref = ref.previousSibling; W.insertBefore(p, convertNodes(nodes), ref ? ref.nextSibling : p.firstChild); },
  after(...nodes) { const p = this.parentNode; if (!p) return; let ref = this.nextSibling; while (ref && nodes.includes(ref)) ref = ref.nextSibling; W.insertBefore(p, convertNodes(nodes), ref); },
  replaceWith(...nodes) { const p = this.parentNode; if (!p) return; let ref = this.nextSibling; while (ref && nodes.includes(ref)) ref = ref.nextSibling; W.remove(this); W.insertBefore(p, convertNodes(nodes), ref); },
};
function mixin(proto, methods) {
  for (const name of Object.getOwnPropertyNames(methods)) {
    const d = Object.getOwnPropertyDescriptor(methods, name);
    d.enumerable = true;
    Object.defineProperty(proto, name, d);
  }
}

// ---------------------------------------------------------------- CharacterData

class CharacterData extends Node {
  get length() { return this.data.length; }
  get textContent() { return this.data; }
  set textContent(v) { this.data = v === null || v === undefined ? '' : String(v); }
  substringData(offset, count) { const d = this.data; if (offset > d.length || offset < 0) throw domError('IndexSizeError', 'The offset is larger than the length.'); return d.substr(offset, count); }
  appendData(s) { this.data = this.data + String(s); }
  insertData(offset, s) { const d = this.data; if (offset > d.length || offset < 0) throw domError('IndexSizeError', 'The offset is larger than the length.'); this.data = d.slice(0, offset) + String(s) + d.slice(offset); }
  deleteData(offset, count) { const d = this.data; if (offset > d.length || offset < 0) throw domError('IndexSizeError', 'The offset is larger than the length.'); this.data = d.slice(0, offset) + d.slice(offset + count); }
  replaceData(offset, count, s) { const d = this.data; if (offset > d.length || offset < 0) throw domError('IndexSizeError', 'The offset is larger than the length.'); this.data = d.slice(0, offset) + String(s) + d.slice(offset + count); }
}
mixin(CharacterData.prototype, childNodeMethods);
Object.defineProperty(CharacterData.prototype, 'nextElementSibling', { get() { let n = this.nextSibling; while (n && n.nodeType !== 1) n = n.nextSibling; return n; }, configurable: true });
Object.defineProperty(CharacterData.prototype, 'previousElementSibling', { get() { let n = this.previousSibling; while (n && n.nodeType !== 1) n = n.previousSibling; return n; }, configurable: true });

class Text extends CharacterData {
  constructor(data) {
    const n = W.createText(data === undefined ? '' : String(data));
    W.setProto(n, new.target.prototype);
    return n;
  }
  get wholeText() {
    let s = this.data, p = this.previousSibling;
    while (p && p.nodeType === 3) { s = p.data + s; p = p.previousSibling; }
    let n = this.nextSibling;
    while (n && n.nodeType === 3) { s = s + n.data; n = n.nextSibling; }
    return s;
  }
  splitText(offset) {
    const d = this.data;
    if (offset > d.length || offset < 0) throw domError('IndexSizeError', 'The offset is larger than the length.');
    const t = W.createText(d.slice(offset));
    this.data = d.slice(0, offset);
    const p = this.parentNode;
    if (p) W.insertBefore(p, t, this.nextSibling);
    return t;
  }
  get assignedSlot() { return null; }
  get [Symbol.toStringTag]() { return 'Text'; }
}
class CDATASection extends Text {}
class Comment extends CharacterData {
  constructor(data) {
    const n = W.createComment(data === undefined ? '' : String(data));
    W.setProto(n, new.target.prototype);
    return n;
  }
  get [Symbol.toStringTag]() { return 'Comment'; }
}
class ProcessingInstruction extends CharacterData {}
class DocumentType extends Node {
  get name() { const i = W.doctypeInfo(this); return i ? i[0] : ''; }
  get publicId() { const i = W.doctypeInfo(this); return i ? i[1] : ''; }
  get systemId() { const i = W.doctypeInfo(this); return i ? i[2] : ''; }
}
mixin(DocumentType.prototype, childNodeMethods);

// ---------------------------------------------------------------- Attr

class Attr extends Node {
  constructor(owner, name) {
    if (!isNode(owner)) throw typeError('Illegal constructor');
    const o = Object.create(new.target.prototype);
    define(o, '_owner', owner);
    define(o, '_name', name);
    return o;
  }
  get nodeType() { return 2; }
  get name() { return this._name; }
  get localName() { return this._name.includes(':') ? this._name.split(':')[1] : this._name; }
  get prefix() { return this._name.includes(':') ? this._name.split(':')[0] : null; }
  get namespaceURI() { return this._name.startsWith('xlink:') ? 'http://www.w3.org/1999/xlink' : this._name.startsWith('xml:') ? 'http://www.w3.org/XML/1998/namespace' : null; }
  get nodeName() { return this._name; }
  get value() { if (!this._owner) return this._detachedValue || ''; const v = W.getAttributeRaw(this._owner, this._name); return v === null ? (this._detachedValue || '') : v; }
  set value(v) { if (this._owner) this._owner.setAttribute(this._name, v); else define(this, '_detachedValue', String(v)); }
  get nodeValue() { return this.value; }
  set nodeValue(v) { this.value = v; }
  get textContent() { return this.value; }
  set textContent(v) { this.value = v; }
  get ownerElement() { return this._owner && W.hasAttribute(this._owner, this._name) ? this._owner : null; }
  get specified() { return true; }
  get parentNode() { return null; }
  get childNodes() { return nodeList([]); }
  get firstChild() { return null; }
  get lastChild() { return null; }
  get isConnected() { return false; }
  get [Symbol.toStringTag]() { return 'Attr'; }
}
const attrCache = new WeakMap();
function attrNode(el, name) {
  let m = attrCache.get(el);
  if (!m) { m = new Map(); attrCache.set(el, m); }
  let a = m.get(name);
  if (!a) { a = new Attr(el, name); m.set(name, a); }
  return a;
}
hooks.attrNode = attrNode;

class NamedNodeMap {
  constructor() { throw typeError('Illegal constructor'); }
  item(i) { const a = W.attrAt(W.partOwner(this), i); return a ? attrNode(W.partOwner(this), a[0]) : null; }
  getNamedItem(name) { const el = W.partOwner(this); return W.hasAttribute(el, name) ? attrNode(el, String(name).toLowerCase()) : null; }
  getNamedItemNS(ns, name) { return this.getNamedItem(name); }
  setNamedItem(attr) { const el = W.partOwner(this); const old = this.getNamedItem(attr.name); el.setAttribute(attr.name, attr.value); define(attr, '_owner', el); return old; }
  setNamedItemNS(attr) { return this.setNamedItem(attr); }
  removeNamedItem(name) { const el = W.partOwner(this); const a = this.getNamedItem(name); if (!a) throw domError('NotFoundError', 'No item with name \'' + name + '\' was found.'); define(a, '_detachedValue', a.value); el.removeAttribute(name); define(a, '_owner', null); return a; }
  [Symbol.iterator]() { const el = W.partOwner(this); const names = el.getAttributeNames(); let i = 0; return { next: () => (i < names.length ? { value: attrNode(el, names[i++]), done: false } : { value: undefined, done: true }), [Symbol.iterator]() { return this; } }; }
  get [Symbol.toStringTag]() { return 'NamedNodeMap'; }
}

// ---------------------------------------------------------------- collections

function defineListIteration(proto, tag) {
  define(proto, 'item', function (i) { i = Number(i) | 0; return i < 0 ? null : (this[i] === undefined ? null : this[i]); });
  define(proto, 'forEach', function (cb, thisArg) { const items = W.listItems(this); for (let i = 0; i < items.length; i++) cb.call(thisArg, items[i], i, this); });
  define(proto, 'entries', function () { return W.listItems(this).entries(); });
  define(proto, 'keys', function () { return W.listItems(this).keys(); });
  define(proto, 'values', function () { return W.listItems(this).values(); });
  define(proto, Symbol.iterator, function () { return W.listItems(this).values(); });
  Object.defineProperty(proto, Symbol.toStringTag, { value: tag, configurable: true });
}
class NodeList { constructor() { throw typeError('Illegal constructor'); } }
defineListIteration(NodeList.prototype, 'NodeList');
class HTMLCollection {
  constructor() { throw typeError('Illegal constructor'); }
  namedItem(name) { name = String(name); const items = W.listItems(this); for (const n of items) if (n.id === name) return n; for (const n of items) if (n.getAttribute('name') === name) return n; return null; }
}
defineListIteration(HTMLCollection.prototype, 'HTMLCollection');
class HTMLFormControlsCollection extends HTMLCollection {
  namedItem(name) { name = String(name); const items = W.listItems(this).filter((n) => n.id === name || n.getAttribute('name') === name); if (items.length === 0) return null; if (items.length === 1) return items[0]; const r = new RadioNodeList(items); return r; }
}
class RadioNodeList {
  constructor(items) { const l = W.staticList(items, 'RadioNodeList'); return l; }
  get value() { for (const n of W.listItems(this)) if (n.checked) return n.value; return ''; }
  set value(v) { for (const n of W.listItems(this)) if (n.value === v) { n.checked = true; return; } }
}
defineListIteration(RadioNodeList.prototype, 'RadioNodeList');
class HTMLOptionsCollection extends HTMLCollection {
  get selectedIndex() { return W.partOwner(this).selectedIndex; }
  set selectedIndex(v) { W.partOwner(this).selectedIndex = v; }
  get length() { return W.listItems(this).length; }
  set length(n) { const s = W.partOwner(this); const items = W.listItems(this); for (let i = items.length - 1; i >= n; i--) W.remove(items[i]); for (let i = items.length; i < n; i++) s.appendChild(document.createElement('option')); }
  add(el, before) { W.partOwner(this).add(el, before); }
  remove(i) { W.partOwner(this).remove(i); }
}
Object.defineProperty(HTMLOptionsCollection.prototype, 'length', { get() { return W.listItems(this).length; }, set(n) { const s = W.partOwner(this); const items = W.listItems(this); for (let i = items.length - 1; i >= n; i--) W.remove(items[i]); for (let i = items.length; i < n; i++) s.appendChild(document.createElement('option')); }, configurable: true });

class DOMTokenList {
  constructor() { throw typeError('Illegal constructor'); }
  _attr() { return W.partAttr(this); }
  _set(tokens) { W.partOwner(this).setAttribute(this._attr(), tokens.join(' ')); }
  _tokens() { return W.tokens(this); }
  contains(t) { return this._tokens().includes(String(t)); }
  add(...ts) { const tokens = this._tokens(); let changed = false; for (let t of ts) { t = String(t); checkToken(t); if (!tokens.includes(t)) { tokens.push(t); changed = true; } } if (changed || W.getAttributeRaw(W.partOwner(this), this._attr()) === null) this._set(tokens); }
  remove(...ts) { const tokens = this._tokens(); let changed = false; for (let t of ts) { t = String(t); checkToken(t); const i = tokens.indexOf(t); if (i >= 0) { tokens.splice(i, 1); changed = true; } } if (changed) this._set(tokens); }
  toggle(t, force) { t = String(t); checkToken(t); const has = this.contains(t); if (has && force !== true) { this.remove(t); return false; } if (!has && force !== false) { this.add(t); return true; } return has; }
  replace(a, b) { a = String(a); b = String(b); checkToken(a); checkToken(b); const tokens = this._tokens(); const i = tokens.indexOf(a); if (i < 0) return false; if (tokens.includes(b)) tokens.splice(i, 1); else tokens[i] = b; this._set(tokens); return true; }
  supports() { return true; }
  item(i) { return this._tokens()[i] === undefined ? null : this._tokens()[i]; }
  get value() { const v = W.getAttributeRaw(W.partOwner(this), this._attr()); return v === null ? '' : v; }
  set value(v) { W.partOwner(this).setAttribute(this._attr(), String(v)); }
  toString() { return this.value; }
  forEach(cb, thisArg) { const t = this._tokens(); for (let i = 0; i < t.length; i++) cb.call(thisArg, t[i], i, this); }
  entries() { return this._tokens().entries(); }
  keys() { return this._tokens().keys(); }
  values() { return this._tokens().values(); }
  [Symbol.iterator]() { return this._tokens().values(); }
  get [Symbol.toStringTag]() { return 'DOMTokenList'; }
}
function checkToken(t) {
  if (t === '') throw domError('SyntaxError', 'The token provided must not be empty.');
  if (/[\t\n\f\r ]/.test(t)) throw domError('InvalidCharacterError', "The token provided ('" + t + "') contains HTML space characters, which are not valid in tokens.");
}
class DOMStringMap { constructor() { throw typeError('Illegal constructor'); } get [Symbol.toStringTag]() { return 'DOMStringMap'; } }

// ---------------------------------------------------------------- DOMRect

class DOMRectReadOnly {
  constructor(x, y, width, height) { this._x = +x || 0; this._y = +y || 0; this._w = +width || 0; this._h = +height || 0; }
  get x() { return this._x; } get y() { return this._y; } get width() { return this._w; } get height() { return this._h; }
  get top() { return Math.min(this._y, this._y + this._h); } get left() { return Math.min(this._x, this._x + this._w); }
  get right() { return Math.max(this._x, this._x + this._w); } get bottom() { return Math.max(this._y, this._y + this._h); }
  toJSON() { return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, right: this.right, bottom: this.bottom, left: this.left }; }
  static fromRect(r) { r = r || {}; return new this(r.x, r.y, r.width, r.height); }
}
class DOMRect extends DOMRectReadOnly {
  set x(v) { this._x = +v; } set y(v) { this._y = +v; } set width(v) { this._w = +v; } set height(v) { this._h = +v; }
  get x() { return this._x; } get y() { return this._y; } get width() { return this._w; } get height() { return this._h; }
}
class DOMRectList {
  constructor(rects) { for (let i = 0; i < rects.length; i++) this[i] = rects[i]; define(this, 'length', rects.length); }
  item(i) { return this[i] || null; }
  [Symbol.iterator]() { return Array.prototype.values.call(this); }
}
class DOMPointReadOnly {
  constructor(x, y, z, w) { this.x = +x || 0; this.y = +y || 0; this.z = +z || 0; this.w = w === undefined ? 1 : +w; }
  matrixTransform() { return new DOMPoint(this.x, this.y, this.z, this.w); }
  toJSON() { return { x: this.x, y: this.y, z: this.z, w: this.w }; }
  static fromPoint(p) { p = p || {}; return new this(p.x, p.y, p.z, p.w); }
}
class DOMPoint extends DOMPointReadOnly {}
const rectOf = (arr) => new DOMRect(arr[0], arr[1], arr[2], arr[3]);

// ---------------------------------------------------------------- traversal

const NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xFFFFFFFF, SHOW_ELEMENT: 1, SHOW_ATTRIBUTE: 2, SHOW_TEXT: 4, SHOW_CDATA_SECTION: 8, SHOW_ENTITY_REFERENCE: 16, SHOW_ENTITY: 32, SHOW_PROCESSING_INSTRUCTION: 64, SHOW_COMMENT: 128, SHOW_DOCUMENT: 256, SHOW_DOCUMENT_TYPE: 512, SHOW_DOCUMENT_FRAGMENT: 1024, SHOW_NOTATION: 2048 };
function acceptNode(walker, node) {
  const mask = 1 << (node.nodeType - 1);
  if (!(walker.whatToShow & mask)) return 3;
  const f = walker.filter;
  if (!f) return 1;
  if (walker._active) throw domError('InvalidStateError', 'Recursive filter.');
  walker._active = true;
  try { return +(typeof f === 'function' ? f(node) : f.acceptNode(node)); } finally { walker._active = false; }
}
class TreeWalker {
  constructor(root, whatToShow, filter) { this.root = root; this.whatToShow = whatToShow === undefined ? 0xFFFFFFFF : whatToShow >>> 0; this.filter = filter || null; this.currentNode = root; this._active = false; }
  parentNode() { let n = this.currentNode; while (n && n !== this.root) { n = n.parentNode; if (n && acceptNode(this, n) === 1) { this.currentNode = n; return n; } } return null; }
  _child(first) {
    let n = first ? this.currentNode.firstChild : this.currentNode.lastChild;
    while (n) {
      const r = acceptNode(this, n);
      if (r === 1) { this.currentNode = n; return n; }
      if (r === 3) { const c = first ? n.firstChild : n.lastChild; if (c) { n = c; continue; } }
      for (;;) {
        const s = first ? n.nextSibling : n.previousSibling;
        if (s) { n = s; break; }
        n = n.parentNode;
        if (!n || n === this.root || n === this.currentNode) return null;
      }
    }
    return null;
  }
  firstChild() { return this._child(true); }
  lastChild() { return this._child(false); }
  _sibling(next) {
    let n = this.currentNode;
    if (n === this.root) return null;
    for (;;) {
      let s = next ? n.nextSibling : n.previousSibling;
      while (s) {
        n = s;
        const r = acceptNode(this, n);
        if (r === 1) { this.currentNode = n; return n; }
        s = next ? n.firstChild : n.lastChild;
        if (r === 2 || !s) s = next ? n.nextSibling : n.previousSibling;
      }
      n = n.parentNode;
      if (!n || n === this.root) return null;
      if (acceptNode(this, n) === 1) return null;
    }
  }
  nextSibling() { return this._sibling(true); }
  previousSibling() { return this._sibling(false); }
  nextNode() {
    let n = this.currentNode, r = 3;
    for (;;) {
      while (r !== 2 && n.firstChild) { n = n.firstChild; r = acceptNode(this, n); if (r === 1) { this.currentNode = n; return n; } }
      let s = null;
      let t = n;
      while (t && t !== this.root) { s = t.nextSibling; if (s) break; t = t.parentNode; }
      if (!s) return null;
      n = s; r = acceptNode(this, n);
      if (r === 1) { this.currentNode = n; return n; }
    }
  }
  previousNode() {
    let n = this.currentNode;
    while (n !== this.root) {
      let s = n.previousSibling;
      while (s) {
        n = s;
        let r = acceptNode(this, n);
        while (r !== 2 && n.lastChild) { n = n.lastChild; r = acceptNode(this, n); }
        if (r === 1) { this.currentNode = n; return n; }
        s = n.previousSibling;
      }
      if (n === this.root || !n.parentNode) return null;
      n = n.parentNode;
      if (acceptNode(this, n) === 1) { this.currentNode = n; return n; }
    }
    return null;
  }
}
class NodeIterator {
  constructor(root, whatToShow, filter) { this.root = root; this.whatToShow = whatToShow === undefined ? 0xFFFFFFFF : whatToShow >>> 0; this.filter = filter || null; this.referenceNode = root; this.pointerBeforeReferenceNode = true; this._active = false; }
  _all() { const out = []; const walk = (n) => { out.push(n); for (let c = n.firstChild; c; c = c.nextSibling) walk(c); }; walk(this.root); return out; }
  nextNode() {
    const all = this._all();
    let i = all.indexOf(this.referenceNode);
    if (!this.pointerBeforeReferenceNode) i++;
    for (; i < all.length; i++) { const n = all[i]; if (acceptNode(this, n) === 1) { this.referenceNode = n; this.pointerBeforeReferenceNode = false; return n; } }
    return null;
  }
  previousNode() {
    const all = this._all();
    let i = all.indexOf(this.referenceNode);
    if (this.pointerBeforeReferenceNode) i--;
    for (; i >= 0; i--) { const n = all[i]; if (acceptNode(this, n) === 1) { this.referenceNode = n; this.pointerBeforeReferenceNode = true; return n; } }
    return null;
  }
  detach() {}
}

// ---------------------------------------------------------------- Range, Selection

function nodeIndex(n) { let i = 0; while ((n = n.previousSibling)) i++; return i; }
function nodeLength(n) { return n.nodeType === 3 || n.nodeType === 8 ? n.data.length : n.childNodes.length; }
function comparePoints(n1, o1, n2, o2) {
  if (n1 === n2) return o1 < o2 ? -1 : o1 > o2 ? 1 : 0;
  const pos = n1.compareDocumentPosition(n2);
  if (pos & 8) { /* n2 contains n1 */ let c = n1; while (c.parentNode !== n2) c = c.parentNode; return nodeIndex(c) < o2 ? -1 : 1; }
  if (pos & 16) { let c = n2; while (c.parentNode !== n1) c = c.parentNode; return nodeIndex(c) < o1 ? 1 : -1; }
  return pos & 4 ? -1 : 1;
}
class AbstractRange {}
class StaticRange extends AbstractRange {
  constructor(init) { super(); this.startContainer = init.startContainer; this.startOffset = init.startOffset; this.endContainer = init.endContainer; this.endOffset = init.endOffset; }
  get collapsed() { return this.startContainer === this.endContainer && this.startOffset === this.endOffset; }
}
class Range extends AbstractRange {
  constructor() { super(); this._sc = document; this._so = 0; this._ec = document; this._eo = 0; }
  get startContainer() { return this._sc; } get startOffset() { return this._so; }
  get endContainer() { return this._ec; } get endOffset() { return this._eo; }
  get collapsed() { return this._sc === this._ec && this._so === this._eo; }
  get commonAncestorContainer() { let a = this._sc; while (a && !(a === this._ec || a.contains(this._ec))) a = a.parentNode; return a; }
  _check(node, offset) { if (!isNode(node)) throw typeError("parameter 1 is not of type 'Node'."); if (offset < 0 || offset > nodeLength(node)) throw domError('IndexSizeError', 'The offset ' + offset + ' is larger than the node\'s length (' + nodeLength(node) + ').'); }
  setStart(node, offset) { this._check(node, offset); this._sc = node; this._so = offset; if (comparePoints(this._sc, this._so, this._ec, this._eo) > 0) { this._ec = node; this._eo = offset; } }
  setEnd(node, offset) { this._check(node, offset); this._ec = node; this._eo = offset; if (comparePoints(this._sc, this._so, this._ec, this._eo) > 0) { this._sc = node; this._so = offset; } }
  setStartBefore(n) { this.setStart(n.parentNode, nodeIndex(n)); }
  setStartAfter(n) { this.setStart(n.parentNode, nodeIndex(n) + 1); }
  setEndBefore(n) { this.setEnd(n.parentNode, nodeIndex(n)); }
  setEndAfter(n) { this.setEnd(n.parentNode, nodeIndex(n) + 1); }
  collapse(toStart) { if (toStart) { this._ec = this._sc; this._eo = this._so; } else { this._sc = this._ec; this._so = this._eo; } }
  selectNode(n) { const p = n.parentNode; if (!p) throw domError('InvalidNodeTypeError', 'The given Node has no parent.'); const i = nodeIndex(n); this._sc = p; this._so = i; this._ec = p; this._eo = i + 1; }
  selectNodeContents(n) { this._sc = n; this._so = 0; this._ec = n; this._eo = nodeLength(n); }
  _nodesIn() {
    const out = [];
    const root = this.commonAncestorContainer;
    const walk = (n) => {
      if (n !== root && this.isPointInRange(n.parentNode, nodeIndex(n)) && this.isPointInRange(n.parentNode, nodeIndex(n) + 1)) { out.push(n); return; }
      for (let c = n.firstChild; c; c = c.nextSibling) walk(c);
    };
    walk(root);
    return out;
  }
  isPointInRange(node, offset) { return comparePoints(this._sc, this._so, node, offset) <= 0 && comparePoints(this._ec, this._eo, node, offset) >= 0; }
  comparePoint(node, offset) { if (comparePoints(node, offset, this._sc, this._so) < 0) return -1; if (comparePoints(node, offset, this._ec, this._eo) > 0) return 1; return 0; }
  intersectsNode(node) { const p = node.parentNode; if (!p) return node === this.commonAncestorContainer || true; const i = nodeIndex(node); return comparePoints(this._sc, this._so, p, i + 1) < 0 && comparePoints(this._ec, this._eo, p, i) > 0; }
  toString() {
    if (this._sc === this._ec && this._sc.nodeType === 3) return this._sc.data.slice(this._so, this._eo);
    let s = '';
    const walk = (n) => {
      if (n.nodeType === 3) {
        const start = n === this._sc ? this._so : 0;
        const end = n === this._ec ? this._eo : n.data.length;
        const p = n.parentNode;
        const idx = p ? nodeIndex(n) : 0;
        const before = comparePoints(p || n, idx, this._sc, this._so) < 0 && n !== this._sc;
        const after = comparePoints(p || n, idx + 1, this._ec, this._eo) > 0 && n !== this._ec;
        if (!before && !after) s += n.data.slice(start, end);
        else if (n === this._sc) s += n.data.slice(start);
        else if (n === this._ec) s += n.data.slice(0, end);
        return;
      }
      for (let c = n.firstChild; c; c = c.nextSibling) walk(c);
    };
    walk(this.commonAncestorContainer);
    return s;
  }
  deleteContents() {
    if (this.collapsed) return;
    if (this._sc === this._ec && this._sc.nodeType === 3) { this._sc.deleteData(this._so, this._eo - this._so); this._eo = this._so; return; }
    for (const n of this._nodesIn()) W.remove(n);
    if (this._sc.nodeType === 3) this._sc.deleteData(this._so, this._sc.data.length - this._so);
    if (this._ec.nodeType === 3) { this._ec.deleteData(0, this._eo); }
    this.collapse(true);
  }
  extractContents() { const frag = this.cloneContents(); this.deleteContents(); return frag; }
  cloneContents() {
    const frag = document.createDocumentFragment();
    if (this.collapsed) return frag;
    if (this._sc === this._ec && this._sc.nodeType === 3) { frag.appendChild(document.createTextNode(this._sc.data.slice(this._so, this._eo))); return frag; }
    const boundary = (n) => n === this._sc || n === this._ec || n.contains(this._sc) || n.contains(this._ec);
    const fully = (n) => { const p = n.parentNode; if (!p) return false; const i = nodeIndex(n); return this.isPointInRange(p, i) && this.isPointInRange(p, i + 1); };
    const cloneInto = (n, into) => {
      for (let c = n.firstChild; c; c = c.nextSibling) {
        if (fully(c)) { into.appendChild(c.cloneNode(true)); continue; }
        if (!boundary(c)) continue;
        if (c.nodeType === 3 || c.nodeType === 8) {
          const start = c === this._sc ? this._so : 0;
          const end = c === this._ec ? this._eo : c.data.length;
          const t = c.cloneNode(false); t.data = c.data.slice(start, end); into.appendChild(t);
          continue;
        }
        const s = c.cloneNode(false);
        into.appendChild(s);
        cloneInto(c, s);
      }
    };
    cloneInto(this.commonAncestorContainer, frag);
    return frag;
  }
  insertNode(node) {
    if (this._sc.nodeType === 3) { const t = this._sc.splitText(this._so); W.insertBefore(this._sc.parentNode, node, t); }
    else W.insertBefore(this._sc, node, this._sc.childNodes[this._so] || null);
  }
  surroundContents(parent) { const frag = this.extractContents(); parent.appendChild(frag); this.insertNode(parent); this.selectNode(parent); }
  cloneRange() { const r = new Range(); r._sc = this._sc; r._so = this._so; r._ec = this._ec; r._eo = this._eo; return r; }
  detach() {}
  createContextualFragment(html) { const ctx = this._sc.nodeType === 1 ? this._sc : (this._sc.parentNode || document.body || document.documentElement); return W.parseFragment(ctx, String(html)); }
  getBoundingClientRect() {
    const rects = this.getClientRects();
    if (rects.length === 0) return new DOMRect(0, 0, 0, 0);
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const r of rects) { x0 = Math.min(x0, r.left); y0 = Math.min(y0, r.top); x1 = Math.max(x1, r.right); y1 = Math.max(y1, r.bottom); }
    return new DOMRect(x0, y0, x1 - x0, y1 - y0);
  }
  getClientRects() {
    const els = new Set();
    const add = (n) => { if (!n) return; if (n.nodeType === 3) n = n.parentNode; if (n && n.nodeType === 1) els.add(n); };
    add(this._sc); add(this._ec);
    for (const n of this._nodesIn()) add(n);
    const out = [];
    for (const el of els) for (const r of W.clientRects(el)) out.push(rectOf(r));
    return new DOMRectList(out);
  }
  get [Symbol.toStringTag]() { return 'Range'; }
}
Range.START_TO_START = 0; Range.START_TO_END = 1; Range.END_TO_END = 2; Range.END_TO_START = 3;
Range.prototype.compareBoundaryPoints = function (how, other) {
  const pairs = [[this._sc, this._so, other._sc, other._so], [this._ec, this._eo, other._sc, other._so], [this._ec, this._eo, other._ec, other._eo], [this._sc, this._so, other._ec, other._eo]];
  const [a, b, c, d] = pairs[how];
  return comparePoints(a, b, c, d);
};

class Selection {
  constructor() { this._ranges = []; }
  get rangeCount() { return this._ranges.length; }
  get type() { return this._ranges.length === 0 ? 'None' : this._ranges[0].collapsed ? 'Caret' : 'Range'; }
  get anchorNode() { return this._ranges.length ? this._ranges[0].startContainer : null; }
  get anchorOffset() { return this._ranges.length ? this._ranges[0].startOffset : 0; }
  get focusNode() { return this._ranges.length ? this._ranges[0].endContainer : null; }
  get focusOffset() { return this._ranges.length ? this._ranges[0].endOffset : 0; }
  get isCollapsed() { return this._ranges.length === 0 || this._ranges[0].collapsed; }
  getRangeAt(i) { if (i < 0 || i >= this._ranges.length) throw domError('IndexSizeError', 'The index provided is outside the range of ranges.'); return this._ranges[i]; }
  addRange(r) { if (this._ranges.length === 0) this._ranges.push(r); }
  removeRange(r) { const i = this._ranges.indexOf(r); if (i >= 0) this._ranges.splice(i, 1); }
  removeAllRanges() { this._ranges = []; }
  empty() { this.removeAllRanges(); }
  collapse(node, offset) { if (node === null) { this.removeAllRanges(); return; } const r = new Range(); r.setStart(node, offset || 0); r.setEnd(node, offset || 0); this._ranges = [r]; }
  setPosition(node, offset) { this.collapse(node, offset); }
  collapseToStart() { if (this._ranges.length) this._ranges[0].collapse(true); }
  collapseToEnd() { if (this._ranges.length) this._ranges[0].collapse(false); }
  extend(node, offset) { if (this._ranges.length) this._ranges[0].setEnd(node, offset || 0); }
  setBaseAndExtent(a, ao, f, fo) { const r = new Range(); r.setStart(a, ao); r.setEnd(f, fo); this._ranges = [r]; }
  selectAllChildren(node) { const r = new Range(); r.selectNodeContents(node); this._ranges = [r]; }
  containsNode(node, partial) { return this._ranges.some((r) => r.intersectsNode(node)) && !!partial; }
  deleteFromDocument() { for (const r of this._ranges) r.deleteContents(); }
  toString() { return this._ranges.map((r) => r.toString()).join(''); }
  get [Symbol.toStringTag]() { return 'Selection'; }
}
const selection = new Selection();

Object.assign(C, { Node, CharacterData, Text, CDATASection, Comment, ProcessingInstruction, DocumentType, Attr, NamedNodeMap, NodeList, HTMLCollection, HTMLFormControlsCollection, RadioNodeList, HTMLOptionsCollection, DOMTokenList, DOMStringMap, DOMRect, DOMRectReadOnly, DOMRectList, DOMPoint, DOMPointReadOnly, NodeFilter, TreeWalker, NodeIterator, AbstractRange, StaticRange, Range, Selection, selection, mixin, parentNodeMethods, childNodeMethods, nodeList, convertNodes, rectOf, attrNode, nodeIndex });
})();
