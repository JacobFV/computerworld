// The browser realm prelude, part 1: the event system.
//
// Runs once per realm as a global script over the `%web` natives. Everything the
// page can see is defined on globalThis at the end (part 5); internal helpers are
// closed over here. The five parts are concatenated in order by the Rust side.
(function () {
'use strict';
const W = globalThis['%web'];
const hooks = {};
Object.defineProperty(globalThis, '%hooks', { value: hooks, writable: true, configurable: true, enumerable: false });

const define = (obj, name, value) => Object.defineProperty(obj, name, { value, writable: true, configurable: true, enumerable: false });
const defineGlobal = (name, value) => define(globalThis, name, value);
const LISTENERS = Symbol('listeners');
const HANDLERS = Symbol('handlers');
const isNode = (v) => v !== null && typeof v === 'object' && W.nodeId(v) !== null;
const nodeId = (v) => W.nodeId(v);

function domError(name, message) {
  return new DOMException(message, name);
}
function typeError(message) {
  return new TypeError(message);
}

// ---------------------------------------------------------------- Event

class Event {
  constructor(type, init) {
    if (arguments.length === 0) throw typeError("Failed to construct 'Event': 1 argument required, but only 0 present.");
    init = init || {};
    this._type = String(type);
    this._bubbles = !!init.bubbles;
    this._cancelable = !!init.cancelable;
    this._composed = !!init.composed;
    this._target = null;
    this._currentTarget = null;
    this._phase = 0;
    this._prevented = false;
    this._stop = false;
    this._stopImmediate = false;
    this._dispatching = false;
    this._path = null;
    this._trusted = false;
    this._timeStamp = performance.now();
  }
  get type() { return this._type; }
  get target() { return this._target; }
  get srcElement() { return this._target; }
  get currentTarget() { return this._currentTarget; }
  get eventPhase() { return this._phase; }
  get bubbles() { return this._bubbles; }
  get cancelable() { return this._cancelable; }
  get composed() { return this._composed; }
  get defaultPrevented() { return this._prevented; }
  get isTrusted() { return this._trusted; }
  get timeStamp() { return this._timeStamp; }
  get cancelBubble() { return this._stop; }
  set cancelBubble(v) { if (v) this._stop = true; }
  get returnValue() { return !this._prevented; }
  set returnValue(v) { if (!v) this.preventDefault(); }
  preventDefault() { if (this._cancelable && !this._passive) this._prevented = true; }
  stopPropagation() { this._stop = true; }
  stopImmediatePropagation() { this._stop = true; this._stopImmediate = true; }
  composedPath() { return this._path ? this._path.slice() : []; }
  initEvent(type, bubbles, cancelable) { this._type = String(type); this._bubbles = !!bubbles; this._cancelable = !!cancelable; }
  get [Symbol.toStringTag]() { return 'Event'; }
}
Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;

class CustomEvent extends Event {
  constructor(type, init) {
    super(type, init);
    this._detail = init && init.detail !== undefined ? init.detail : null;
  }
  get detail() { return this._detail; }
  initCustomEvent(type, bubbles, cancelable, detail) { this.initEvent(type, bubbles, cancelable); this._detail = detail; }
}

class UIEvent extends Event {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this._detail = init.detail | 0;
    this._view = init.view || null;
    this.which = init.which | 0;
  }
  get detail() { return this._detail; }
  get view() { return this._view; }
  initUIEvent(type, bubbles, cancelable, view, detail) { this.initEvent(type, bubbles, cancelable); this._view = view || null; this._detail = detail | 0; }
}

const modifierKeys = ['ctrlKey', 'shiftKey', 'altKey', 'metaKey'];
function copyModifiers(ev, init) {
  for (const k of modifierKeys) ev[k] = !!init[k];
  ev.getModifierState = function (key) {
    switch (key) {
      case 'Control': return this.ctrlKey;
      case 'Shift': return this.shiftKey;
      case 'Alt': return this.altKey;
      case 'Meta': return this.metaKey;
      default: return false;
    }
  };
}

class MouseEvent extends UIEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.screenX = init.screenX || 0;
    this.screenY = init.screenY || 0;
    this.clientX = init.clientX || 0;
    this.clientY = init.clientY || 0;
    this.pageX = init.pageX !== undefined ? init.pageX : this.clientX + (globalThis.scrollX || 0);
    this.pageY = init.pageY !== undefined ? init.pageY : this.clientY + (globalThis.scrollY || 0);
    this.x = this.clientX;
    this.y = this.clientY;
    this.button = init.button | 0;
    this.buttons = init.buttons | 0;
    this.relatedTarget = init.relatedTarget || null;
    this.movementX = init.movementX || 0;
    this.movementY = init.movementY || 0;
    this._offset = init.offsetX !== undefined ? [init.offsetX, init.offsetY] : null;
    copyModifiers(this, init);
  }
  get offsetX() { if (this._offset) return this._offset[0]; const t = this._target; if (!isNode(t)) return this.clientX; const r = W.boundingRect(t); return this.clientX - r[0]; }
  get offsetY() { if (this._offset) return this._offset[1]; const t = this._target; if (!isNode(t)) return this.clientY; const r = W.boundingRect(t); return this.clientY - r[1]; }
  get layerX() { return this.offsetX; }
  get layerY() { return this.offsetY; }
  initMouseEvent(type, bubbles, cancelable, view, detail, sx, sy, cx, cy, ctrl, alt, shift, meta, button, related) {
    this.initUIEvent(type, bubbles, cancelable, view, detail);
    this.screenX = sx; this.screenY = sy; this.clientX = cx; this.clientY = cy;
    this.ctrlKey = !!ctrl; this.altKey = !!alt; this.shiftKey = !!shift; this.metaKey = !!meta;
    this.button = button | 0; this.relatedTarget = related || null;
  }
}

class PointerEvent extends MouseEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.pointerId = init.pointerId !== undefined ? init.pointerId : 1;
    this.pointerType = init.pointerType || 'mouse';
    this.isPrimary = init.isPrimary !== undefined ? !!init.isPrimary : true;
    this.width = init.width || 1;
    this.height = init.height || 1;
    this.pressure = init.pressure || 0;
    this.tiltX = 0; this.tiltY = 0; this.twist = 0;
  }
  getCoalescedEvents() { return [this]; }
  getPredictedEvents() { return []; }
}

class WheelEvent extends MouseEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.deltaX = init.deltaX || 0;
    this.deltaY = init.deltaY || 0;
    this.deltaZ = init.deltaZ || 0;
    this.deltaMode = init.deltaMode | 0;
  }
}
WheelEvent.DOM_DELTA_PIXEL = 0; WheelEvent.DOM_DELTA_LINE = 1; WheelEvent.DOM_DELTA_PAGE = 2;

class DragEvent extends MouseEvent {
  constructor(type, init) { super(type, init); this.dataTransfer = (init && init.dataTransfer) || null; }
}

const keyCodes = { Backspace: 8, Tab: 9, Enter: 13, Shift: 16, Control: 17, Alt: 18, Pause: 19, CapsLock: 20, Escape: 27, ' ': 32, PageUp: 33, PageDown: 34, End: 35, Home: 36, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Insert: 45, Delete: 46, Meta: 91, ContextMenu: 93, F1: 112, F2: 113, F3: 114, F4: 115, F5: 116, F6: 117, F7: 118, F8: 119, F9: 120, F10: 121, F11: 122, F12: 123 };
function keyCodeOf(key) {
  if (key in keyCodes) return keyCodes[key];
  if (key.length === 1) {
    const c = key.toUpperCase().charCodeAt(0);
    if (c >= 48 && c <= 90) return c;
    const punct = { ';': 186, '=': 187, ',': 188, '-': 189, '.': 190, '/': 191, '`': 192, '[': 219, '\\': 220, ']': 221, "'": 222 };
    return punct[key] || c;
  }
  return 0;
}

class KeyboardEvent extends UIEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.key = init.key || '';
    this.code = init.code || '';
    this.location = init.location | 0;
    this.repeat = !!init.repeat;
    this.isComposing = !!init.isComposing;
    this.charCode = init.charCode !== undefined ? init.charCode : (type === 'keypress' && this.key.length === 1 ? this.key.charCodeAt(0) : 0);
    this.keyCode = init.keyCode !== undefined ? init.keyCode : (type === 'keypress' && this.key.length === 1 ? this.charCode : keyCodeOf(this.key));
    this.which = init.which !== undefined ? init.which : this.keyCode;
    copyModifiers(this, init);
  }
}
KeyboardEvent.DOM_KEY_LOCATION_STANDARD = 0; KeyboardEvent.DOM_KEY_LOCATION_LEFT = 1; KeyboardEvent.DOM_KEY_LOCATION_RIGHT = 2; KeyboardEvent.DOM_KEY_LOCATION_NUMPAD = 3;

class InputEvent extends UIEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.data = init.data !== undefined ? init.data : null;
    this.inputType = init.inputType || '';
    this.isComposing = !!init.isComposing;
    this.dataTransfer = init.dataTransfer || null;
  }
  getTargetRanges() { return []; }
}

class FocusEvent extends UIEvent {
  constructor(type, init) { super(type, init); this.relatedTarget = (init && init.relatedTarget) || null; }
}
class CompositionEvent extends UIEvent {
  constructor(type, init) { super(type, init); this.data = (init && init.data) || ''; }
}
class ProgressEvent extends Event {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.lengthComputable = !!init.lengthComputable;
    this.loaded = init.loaded || 0;
    this.total = init.total || 0;
  }
}
class PopStateEvent extends Event {
  constructor(type, init) { super(type, init); this.state = init && init.state !== undefined ? init.state : null; }
}
class HashChangeEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.oldURL = init.oldURL || ''; this.newURL = init.newURL || ''; }
}
class PageTransitionEvent extends Event {
  constructor(type, init) { super(type, init); this.persisted = !!(init && init.persisted); }
}
class BeforeUnloadEvent extends Event {
  constructor() { super('beforeunload', { cancelable: true }); this._rv = ''; }
  get returnValue() { return this._rv; }
  set returnValue(v) { this._rv = String(v); if (v) this._prevented = true; }
}
class SubmitEvent extends Event {
  constructor(type, init) { super(type, init); this.submitter = (init && init.submitter) || null; }
}
class FormDataEvent extends Event {
  constructor(type, init) { super(type, init); this.formData = (init && init.formData) || null; }
}
class TransitionEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.propertyName = init.propertyName || ''; this.elapsedTime = init.elapsedTime || 0; this.pseudoElement = init.pseudoElement || ''; }
}
class AnimationEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.animationName = init.animationName || ''; this.elapsedTime = init.elapsedTime || 0; this.pseudoElement = init.pseudoElement || ''; }
}
class ErrorEvent extends Event {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.message = init.message || '';
    this.filename = init.filename || '';
    this.lineno = init.lineno || 0;
    this.colno = init.colno || 0;
    this.error = init.error !== undefined ? init.error : null;
  }
}
class PromiseRejectionEvent extends Event {
  constructor(type, init) {
    super(type, init);
    if (!init || !('promise' in init)) throw typeError("Failed to construct 'PromiseRejectionEvent': required member promise is undefined.");
    this.promise = init.promise;
    this.reason = init.reason;
  }
}
class MessageEvent extends Event {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.data = init.data !== undefined ? init.data : null;
    this.origin = init.origin || '';
    this.lastEventId = init.lastEventId || '';
    this.source = init.source || null;
    this.ports = init.ports || [];
  }
}
class StorageEvent extends Event {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.key = init.key !== undefined ? init.key : null;
    this.oldValue = init.oldValue !== undefined ? init.oldValue : null;
    this.newValue = init.newValue !== undefined ? init.newValue : null;
    this.url = init.url || '';
    this.storageArea = init.storageArea || null;
  }
}
class ClipboardEvent extends Event {
  constructor(type, init) { super(type, init); this.clipboardData = (init && init.clipboardData) || null; }
}
class TouchEvent extends UIEvent {
  constructor(type, init) {
    super(type, init);
    init = init || {};
    this.touches = init.touches || [];
    this.targetTouches = init.targetTouches || [];
    this.changedTouches = init.changedTouches || [];
    copyModifiers(this, init);
  }
}
class ToggleEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.oldState = init.oldState || ''; this.newState = init.newState || ''; }
}
class CloseEvent extends Event {
  constructor(type, init) { super(type, init); init = init || {}; this.wasClean = !!init.wasClean; this.code = init.code | 0; this.reason = init.reason || ''; }
}
class SecurityPolicyViolationEvent extends Event {}

// ---------------------------------------------------------------- EventTarget

function listenerMap(target) {
  let m = target[LISTENERS];
  if (!m) {
    m = new Map();
    define(target, LISTENERS, m);
  }
  return m;
}

function normalizeOptions(options) {
  if (typeof options === 'boolean') return { capture: options, once: false, passive: false, signal: null };
  if (options === null || typeof options !== 'object') return { capture: !!options, once: false, passive: false, signal: null };
  return { capture: !!options.capture, once: !!options.once, passive: !!options.passive, signal: options.signal || null };
}

class EventTarget {
  addEventListener(type, callback, options) {
    if (this === undefined || this === null) return EventTarget.prototype.addEventListener.call(globalThis, type, callback, options);
    if (arguments.length < 2) throw typeError("Failed to execute 'addEventListener' on 'EventTarget': 2 arguments required, but only " + arguments.length + " present.");
    if (callback === null || callback === undefined) return;
    if (typeof callback !== 'function' && typeof callback !== 'object') throw typeError("Failed to execute 'addEventListener' on 'EventTarget': parameter 2 is not of type 'Object'.");
    const o = normalizeOptions(options);
    if (o.signal && o.signal.aborted) return;
    type = String(type);
    const m = listenerMap(this);
    let list = m.get(type);
    if (!list) { list = []; m.set(type, list); }
    for (const l of list) if (l.callback === callback && l.capture === o.capture) return;
    const entry = { callback, capture: o.capture, once: o.once, passive: o.passive, removed: false };
    list.push(entry);
    if (o.signal) o.signal.addEventListener('abort', () => this.removeEventListener(type, callback, o.capture));
  }
  removeEventListener(type, callback, options) {
    if (this === undefined || this === null) return EventTarget.prototype.removeEventListener.call(globalThis, type, callback, options);
    const m = this[LISTENERS];
    if (!m) return;
    const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
    const list = m.get(String(type));
    if (!list) return;
    for (let i = 0; i < list.length; i++) {
      if (list[i].callback === callback && list[i].capture === capture) { list[i].removed = true; list.splice(i, 1); return; }
    }
  }
  dispatchEvent(event) {
    if (this === undefined || this === null) return EventTarget.prototype.dispatchEvent.call(globalThis, event);
    if (!(event instanceof Event)) throw typeError("Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'.");
    if (event._dispatching) throw domError('InvalidStateError', "Failed to execute 'dispatchEvent' on 'EventTarget': The event is already being dispatched.");
    event._trusted = false;
    return dispatch(this, event);
  }
  get [Symbol.toStringTag]() { return 'EventTarget'; }
}

// Event handler IDL attributes (`onclick`): one slot per type, run in listener
// order at the position it was first set.
function handlerMap(target) {
  let m = target[HANDLERS];
  if (!m) { m = new Map(); define(target, HANDLERS, m); }
  return m;
}
function getHandler(target, type) {
  const m = target[HANDLERS];
  const h = m && m.get(type);
  if (!h && isNode(target) && W.hasAttribute(target, 'on' + type)) return compileHandler(target, type);
  if (h && h.attr && isNode(target)) {
    // Compiled from a content attribute the first time it is read.
    if (h.source !== W.getAttributeRaw(target, 'on' + type)) return compileHandler(target, type);
  }
  return h ? h.fn : null;
}
function compileHandler(target, type) {
  const src = W.getAttributeRaw(target, 'on' + type);
  const m = handlerMap(target);
  if (src === null) { const old = m.get(type); if (old && old.attr) { old.fn = null; old.attr = false; old.source = undefined; } return null; }
  let fn;
  try { fn = new Function('event', src); } catch (e) { reportError(e); return null; }
  setHandler(target, type, fn, true, src);
  return fn;
}
function setHandler(target, type, fn, attr, source) {
  const m = handlerMap(target);
  const old = m.get(type);
  if (typeof fn !== 'function' && !(typeof fn === 'object' && fn !== null)) fn = null;
  if (old) {
    // The slot stays (with its listener position) even when cleared, so a content
    // attribute is not recompiled over an explicit `on* = null`.
    old.fn = fn; old.attr = !!attr; old.source = source;
    return;
  }
  if (!fn) return;
  const entry = { fn, attr: !!attr, source, listener: null };
  entry.listener = function (ev) {
    const f = entry.fn;
    if (!f) return;
    let r;
    if (ev instanceof ErrorEvent && ev.type === 'error' && target === globalThis) r = f.call(target, ev.message, ev.filename, ev.lineno, ev.colno, ev.error);
    else if (typeof f === 'function') r = f.call(target, ev);
    else if (typeof f.handleEvent === 'function') r = f.handleEvent(ev);
    if (ev instanceof BeforeUnloadEvent) { if (r !== undefined && r !== null) ev.returnValue = r; }
    else if (r === false || (ev.type === 'error' && target === globalThis && r === true)) ev.preventDefault();
  };
  m.set(type, entry);
  target.addEventListener(type, entry.listener, false);
}
function defineHandlerAttr(proto, type) {
  Object.defineProperty(proto, 'on' + type, {
    get() { return getHandler(this, type); },
    set(v) { setHandler(this, type, v, false); },
    configurable: true, enumerable: true,
  });
}
const globalEventTypes = ['abort', 'animationcancel', 'animationend', 'animationiteration', 'animationstart', 'auxclick', 'beforeinput', 'beforetoggle', 'blur', 'cancel', 'canplay', 'canplaythrough', 'change', 'click', 'close', 'contextmenu', 'copy', 'cuechange', 'cut', 'dblclick', 'drag', 'dragend', 'dragenter', 'dragleave', 'dragover', 'dragstart', 'drop', 'durationchange', 'emptied', 'ended', 'error', 'focus', 'focusin', 'focusout', 'formdata', 'gotpointercapture', 'input', 'invalid', 'keydown', 'keypress', 'keyup', 'load', 'loadeddata', 'loadedmetadata', 'loadstart', 'lostpointercapture', 'mousedown', 'mouseenter', 'mouseleave', 'mousemove', 'mouseout', 'mouseover', 'mouseup', 'mousewheel', 'paste', 'pause', 'play', 'playing', 'pointercancel', 'pointerdown', 'pointerenter', 'pointerleave', 'pointermove', 'pointerout', 'pointerover', 'pointerup', 'progress', 'ratechange', 'reset', 'resize', 'scroll', 'scrollend', 'securitypolicyviolation', 'seeked', 'seeking', 'select', 'selectionchange', 'selectstart', 'slotchange', 'stalled', 'submit', 'suspend', 'timeupdate', 'toggle', 'touchcancel', 'touchend', 'touchmove', 'touchstart', 'transitioncancel', 'transitionend', 'transitionrun', 'transitionstart', 'volumechange', 'waiting', 'wheel', 'webkitanimationend', 'webkitanimationiteration', 'webkitanimationstart', 'webkittransitionend'];
const windowEventTypes = ['afterprint', 'beforeprint', 'beforeunload', 'hashchange', 'languagechange', 'message', 'messageerror', 'offline', 'online', 'pagehide', 'pageshow', 'popstate', 'rejectionhandled', 'storage', 'unhandledrejection', 'unload', 'gamepadconnected', 'gamepaddisconnected'];
const documentEventTypes = ['readystatechange', 'visibilitychange', 'DOMContentLoaded', 'fullscreenchange', 'fullscreenerror', 'pointerlockchange', 'pointerlockerror'];

// The event path: the target, its ancestors (shadow hosts included, template
// contents excluded), the document, then the window.
function eventPath(target, composed) {
  const path = [target];
  if (isNode(target)) {
    let cur = target;
    for (;;) {
      let p = cur.parentNode;
      if (p === null) {
        const host = cur[SHADOW_HOST];
        if (host && (composed || true)) { p = host; } else break;
      }
      path.push(p);
      cur = p;
    }
    if (cur === document) path.push(globalThis);
  }
  return path;
}
const SHADOW_HOST = Symbol('shadowHost');

// Counts `on*` attributes set by script, so a dispatch that began with none in
// the document notices one added while it runs.
let inlineEpoch = 0;
hooks.inlineHandlerSet = () => { inlineEpoch++; };
function invokeListeners(target, event, phase) {
  // With no `on*` attribute anywhere in the document (`_inline` false, measured
  // when the dispatch began, and none set since) and no handler slot here, there
  // is no inline handler to compile or refresh.
  if (event._inline !== false || event._inlineEpoch !== inlineEpoch || target[HANDLERS] !== undefined) maybeInlineHandler(target, event);
  const m = target[LISTENERS];
  if (!m) return;
  const list = m.get(event._type);
  if (!list || list.length === 0) return;
  const snapshot = list.slice();
  event._currentTarget = target;
  for (const entry of snapshot) {
    if (entry.removed) continue;
    if (phase === 1 && !entry.capture) continue;
    if (phase === 3 && entry.capture) continue;
    if (entry.once) target.removeEventListener(event._type, entry.callback, entry.capture);
    event._passive = entry.passive;
    try {
      const cb = entry.callback;
      if (typeof cb === 'function') cb.call(target, event);
      else if (cb && typeof cb.handleEvent === 'function') cb.handleEvent(event);
    } catch (e) {
      reportError(e);
    }
    event._passive = false;
    if (event._stopImmediate) break;
  }
}
function maybeInlineHandler(target, event) {
  if (!isNode(target)) return;
  const m = target[HANDLERS];
  const h = m && m.get(event._type);
  if (h) {
    // A content-attribute handler follows the attribute's current text.
    if (h.attr && h.source !== W.getAttributeRaw(target, 'on' + event._type)) compileHandler(target, event._type);
    return;
  }
  if (W.hasAttribute(target, 'on' + event._type)) compileHandler(target, event._type);
}
Object.defineProperty(EventTarget.prototype, '%invoke', { value: invokeListeners, configurable: true });

function dispatch(target, event) {
  event._dispatching = true;
  event._target = event._targetOverride || target;
  event._stop = false;
  event._stopImmediate = false;
  event._inline = W.inlineHandlers();
  event._inlineEpoch = inlineEpoch;
  const path = eventPath(target, event._composed);
  event._path = path;
  // Capture phase, then target, then bubble.
  for (let i = path.length - 1; i >= 1; i--) {
    if (event._stop) break;
    event._phase = 1;
    invokeListeners(path[i], event, 1);
  }
  if (!event._stop) {
    event._phase = 2;
    invokeListeners(path[0], event, 1);
    if (!event._stopImmediate) invokeListeners(path[0], event, 3);
  }
  if (event._bubbles) {
    for (let i = 1; i < path.length; i++) {
      if (event._stop) break;
      event._phase = 3;
      invokeListeners(path[i], event, 3);
    }
  }
  event._phase = 0;
  event._currentTarget = null;
  event._dispatching = false;
  return !event._prevented;
}

// A trusted event from the browser.
function fire(target, event) {
  event._trusted = true;
  return dispatch(target, event);
}
function fireSimple(target, type, bubbles, cancelable) {
  return fire(target, new Event(type, { bubbles, cancelable }));
}

// Uncaught exceptions: `error` on window, then the console.
function reportError(err) {
  let message = 'Uncaught ' + describeError(err);
  let handled = false;
  try {
    const ev = new ErrorEvent('error', { message: describeError(err), error: err, cancelable: true, filename: (err && err.fileName) || '' });
    handled = !fire(globalThis, ev);
  } catch (e) { /* ignore */ }
  if (!handled) {
    const stack = err && typeof err === 'object' && typeof err.stack === 'string' ? err.stack : null;
    W.log('error', stack && stack.indexOf(String(err.message)) >= 0 ? 'Uncaught ' + stack : message);
  }
}
function describeError(err) {
  if (err instanceof Error) return err.name + ': ' + err.message;
  try { return String(err); } catch (e) { return 'exception'; }
}
hooks.uncaught = (err) => reportError(err);
hooks.unhandledRejection = (reason, promise) => {
  let handled = false;
  try { handled = !fire(globalThis, new PromiseRejectionEvent('unhandledrejection', { promise, reason, cancelable: true })); } catch (e) { /* ignore */ }
  if (!handled) {
    const stack = reason && typeof reason === 'object' && typeof reason.stack === 'string' ? reason.stack : null;
    W.log('error', 'Uncaught (in promise) ' + (stack || describeError(reason)));
  }
};

globalThis['%core'] = { W, hooks, define, defineGlobal, isNode, nodeId, domError, typeError, LISTENERS, HANDLERS, SHADOW_HOST, Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, WheelEvent, DragEvent, KeyboardEvent, InputEvent, FocusEvent, CompositionEvent, ProgressEvent, PopStateEvent, HashChangeEvent, PageTransitionEvent, BeforeUnloadEvent, SubmitEvent, FormDataEvent, TransitionEvent, AnimationEvent, ErrorEvent, PromiseRejectionEvent, MessageEvent, StorageEvent, ClipboardEvent, TouchEvent, ToggleEvent, CloseEvent, SecurityPolicyViolationEvent, EventTarget, defineHandlerAttr, globalEventTypes, windowEventTypes, documentEventTypes, dispatch, fire, fireSimple, reportError, describeError, setHandler, getHandler };
})();
