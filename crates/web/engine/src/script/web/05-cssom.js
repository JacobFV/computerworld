// Prelude part 5: the CSSOM, computed style, media queries.
(function () {
'use strict';
const C = globalThis['%core'];
const { W, define, domError, typeError, EventTarget, fire } = C;

// ---------------------------------------------------------------- CSSStyleDeclaration

class CSSStyleDeclaration {
  constructor() { throw typeError('Illegal constructor'); }
  getPropertyValue(name) { return W.declOp(this, 'get', String(name)); }
  getPropertyPriority(name) { return W.declOp(this, 'priority', String(name)); }
  setProperty(name, value, priority) { W.declOp(this, 'set', String(name), value === null || value === undefined ? '' : String(value), priority === undefined || priority === null ? '' : String(priority)); }
  removeProperty(name) { const old = W.declOp(this, 'get', String(name)); W.declOp(this, 'remove', String(name)); return old; }
  item(i) { return W.declOp(this, 'item', String(i | 0)) || ''; }
  get length() { return W.declOp(this, 'length', ''); }
  get cssText() { return W.declOp(this, 'cssText', ''); }
  set cssText(v) { W.declOp(this, 'setCssText', String(v)); }
  get parentRule() { return this['%parentRule'] || null; }
  [Symbol.iterator]() { const n = this.length; const out = []; for (let i = 0; i < n; i++) out.push(this.item(i)); return out.values(); }
  get [Symbol.toStringTag]() { return 'CSSStyleDeclaration'; }
}

function getComputedStyle(el, pseudo) {
  if (!C.isNode(el) || el.nodeType !== 1) throw typeError("Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.");
  return W.computedStyle(el, pseudo === undefined || pseudo === null ? '' : String(pseudo));
}

// ---------------------------------------------------------------- rules

class CSSRule {
  constructor() { throw typeError('Illegal constructor'); }
  get parentStyleSheet() { return sheetById(W.nodeId(this) === null ? this['%sheet'] : 0); }
  get parentRule() { return this['%parent'] || null; }
  get type() { return this._info().type; }
  get cssText() { return this._info().cssText; }
  _info() { return W.sheetOp(this['%sheet'], 'rule', this['%parentPath'], this['%index']); }
  get [Symbol.toStringTag]() { return 'CSSRule'; }
}
for (const [k, v] of Object.entries({ STYLE_RULE: 1, CHARSET_RULE: 2, IMPORT_RULE: 3, MEDIA_RULE: 4, FONT_FACE_RULE: 5, PAGE_RULE: 6, KEYFRAMES_RULE: 7, KEYFRAME_RULE: 8, NAMESPACE_RULE: 10, SUPPORTS_RULE: 12 })) { Object.defineProperty(CSSRule, k, { value: v, enumerable: true }); Object.defineProperty(CSSRule.prototype, k, { value: v, enumerable: true }); }

class CSSStyleRule extends CSSRule {
  get selectorText() { return this._info().selectorText; }
  set selectorText(v) { W.sheetOp(this['%sheet'], 'setSelector', this['%path'], String(v)); }
  get style() { let s = this['%style']; if (!s) { s = W.sheetOp(this['%sheet'], 'ruleStyle', this['%path']); define(s, '%parentRule', this); define(this, '%style', s); } return s; }
  get styleMap() { const st = this.style; return { get: (p) => ({ toString: () => st.getPropertyValue(p) }), set: (p, v) => st.setProperty(p, String(v)) }; }
  get cssRules() { return new CSSRuleList(this['%sheet'], this['%path'], this); }
}
class CSSGroupingRule extends CSSRule {
  get cssRules() { return new CSSRuleList(this['%sheet'], this['%path'], this); }
  insertRule(text, index) { return W.sheetOp(this['%sheet'], 'insert', this['%path'], String(text), index === undefined ? 0 : index | 0); }
  deleteRule(index) { W.sheetOp(this['%sheet'], 'delete', this['%path'], index | 0); }
}
class CSSConditionRule extends CSSGroupingRule { get conditionText() { return this._info().conditionText; } }
class CSSMediaRule extends CSSConditionRule { get media() { return new MediaList(this._info().conditionText, (v) => {}); } }
class CSSSupportsRule extends CSSConditionRule {}
class CSSLayerBlockRule extends CSSGroupingRule { get name() { return this._info().name; } }
class CSSLayerStatementRule extends CSSRule { get nameList() { return this._info().name.split(',').map((s) => s.trim()); } }
class CSSImportRule extends CSSRule { get href() { return this._info().name; } get media() { return new MediaList(this._info().conditionText, () => {}); } get styleSheet() { return null; } get layerName() { return null; } }
class CSSFontFaceRule extends CSSRule { get style() { let s = this['%style']; if (!s) { s = W.sheetOp(this['%sheet'], 'ruleStyle', this['%path']); define(this, '%style', s); } return s; } }
class CSSPageRule extends CSSGroupingRule { get selectorText() { return ''; } get style() { let s = this['%style']; if (!s) { s = W.sheetOp(this['%sheet'], 'ruleStyle', this['%path']); define(this, '%style', s); } return s; } }
class CSSNamespaceRule extends CSSRule { get namespaceURI() { return ''; } get prefix() { return ''; } }
class CSSKeyframeRule extends CSSRule {
  constructor() { super(); }
  get keyText() { return this._text.split('{')[0].trim(); }
  get style() { return { cssText: this._text.slice(this._text.indexOf('{') + 1, this._text.lastIndexOf('}')).trim(), getPropertyValue: (p) => { const m = new RegExp('(?:^|;)\\s*' + p + '\\s*:\\s*([^;]*)').exec(this.style.cssText); return m ? m[1].trim() : ''; } }; }
  get cssText() { return this._text; }
}
class CSSKeyframesRule extends CSSRule {
  get name() { return this._info().name; }
  get cssRules() { const frames = W.sheetOp(this['%sheet'], 'keyframes', this['%path']); const list = frames.map((t) => { const r = Object.create(CSSKeyframeRule.prototype); define(r, '_text', t); return r; }); return C.makeStaticRuleList(list); }
  findRule(sel) { return Array.from(this.cssRules).find((r) => r.keyText === sel) || null; }
  appendRule() {} deleteRule() {}
  get length() { return this.cssRules.length; }
}
class CSSUnknownRule extends CSSRule {}
const ruleClasses = { 1: CSSStyleRule, 3: CSSImportRule, 4: CSSMediaRule, 5: CSSFontFaceRule, 6: CSSPageRule, 7: CSSKeyframesRule, 10: CSSNamespaceRule, 12: CSSSupportsRule, 0: CSSLayerBlockRule };
function ruleObject(sheetId, parentPath, index, parent) {
  const info = W.sheetOp(sheetId, 'rule', parentPath, index);
  if (!info) return null;
  let cls = ruleClasses[info.type] || CSSUnknownRule;
  if (info.type === 0 && info.cssText.startsWith('@layer') && info.cssText.endsWith(';')) cls = CSSLayerStatementRule;
  if (info.type === 0 && !info.cssText.startsWith('@layer')) cls = CSSUnknownRule;
  const r = Object.create(cls.prototype);
  define(r, '%sheet', sheetId);
  define(r, '%parentPath', parentPath);
  define(r, '%index', index);
  define(r, '%path', info.path);
  define(r, '%parent', parent || null);
  return r;
}
class CSSRuleList {
  constructor(sheetId, path, parent) {
    const arr = [];
    const n = W.sheetOp(sheetId, 'count', path);
    for (let i = 0; i < n; i++) arr.push(ruleObject(sheetId, path, i, parent));
    const list = W.staticList(arr, 'CSSRuleList');
    return list;
  }
  item(i) { return this[i] === undefined ? null : this[i]; }
  [Symbol.iterator]() { return W.listItems(this).values(); }
  get [Symbol.toStringTag]() { return 'CSSRuleList'; }
}
C.makeStaticRuleList = (arr) => W.staticList(arr, 'CSSRuleList');

// ---------------------------------------------------------------- sheets

class MediaList {
  constructor(text, onChange) { this._text = text || ''; this._onChange = onChange; }
  get mediaText() { return this._text; }
  set mediaText(v) { this._text = String(v); this._onChange(this._text); }
  get length() { return this._text ? this._text.split(',').length : 0; }
  item(i) { const parts = this._text ? this._text.split(',').map((s) => s.trim()) : []; return parts[i] === undefined ? null : parts[i]; }
  appendMedium(m) { const parts = this._text ? this._text.split(',').map((s) => s.trim()) : []; if (!parts.includes(m)) parts.push(m); this.mediaText = parts.join(', '); }
  deleteMedium(m) { const parts = this._text ? this._text.split(',').map((s) => s.trim()) : []; this.mediaText = parts.filter((p) => p !== m).join(', '); }
  toString() { return this._text; }
  [Symbol.iterator]() { return (this._text ? this._text.split(',').map((s) => s.trim()) : []).values(); }
}
class StyleSheet {
  constructor() { throw typeError('Illegal constructor'); }
  get type() { return 'text/css'; }
  get href() { const i = W.sheetOp(this['%id'], 'info'); return i ? i.href : null; }
  get ownerNode() { const i = W.sheetOp(this['%id'], 'info'); return i ? i.ownerNode : null; }
  get parentStyleSheet() { return null; }
  get title() { const o = this.ownerNode; return o ? o.getAttribute('title') : null; }
  get media() { const i = W.sheetOp(this['%id'], 'info'); return new MediaList(i ? i.media : '', () => {}); }
  get disabled() { const i = W.sheetOp(this['%id'], 'info'); return i ? i.disabled : false; }
  set disabled(v) { W.sheetOp(this['%id'], 'setDisabled', !!v); }
}
const sheets = new Map();
class CSSStyleSheet extends StyleSheet {
  constructor(options) {
    const id = W.sheetOp(0, 'new', '');
    const s = Object.create(new.target.prototype);
    define(s, '%id', id);
    define(s, '%constructed', true);
    sheets.set(id, s);
    if (options && options.disabled) s.disabled = true;
    return s;
  }
  get cssRules() { return new CSSRuleList(this['%id'], '', null); }
  get rules() { return this.cssRules; }
  get ownerRule() { return null; }
  insertRule(text, index) { return W.sheetOp(this['%id'], 'insert', '', String(text), index === undefined ? 0 : index | 0); }
  deleteRule(index) { W.sheetOp(this['%id'], 'delete', '', index | 0); }
  addRule(selector, style, index) { const n = this.cssRules.length; this.insertRule(selector + ' { ' + style + ' }', index === undefined ? n : index); return -1; }
  removeRule(index) { this.deleteRule(index === undefined ? 0 : index); }
  replaceSync(text) { if (!this['%constructed']) throw domError('NotAllowedError', "Failed to execute 'replaceSync' on 'CSSStyleSheet': Can't call replaceSync on non-constructed CSSStyleSheets."); W.sheetOp(this['%id'], 'replace', String(text)); }
  replace(text) { try { this.replaceSync(text); return Promise.resolve(this); } catch (e) { return Promise.reject(e); } }
  get [Symbol.toStringTag]() { return 'CSSStyleSheet'; }
}
function sheetById(id) {
  if (id === null || id === undefined) return null;
  let s = sheets.get(id);
  if (!s) { s = Object.create(CSSStyleSheet.prototype); define(s, '%id', id); define(s, '%constructed', false); sheets.set(id, s); }
  return s;
}
Object.defineProperty(CSSRule.prototype, 'parentStyleSheet', { get() { return sheetById(this['%sheet']); }, configurable: true });
class StyleSheetList {
  constructor(ids) { return W.staticList(ids.map(sheetById), 'StyleSheetList'); }
  item(i) { return this[i] === undefined ? null : this[i]; }
  [Symbol.iterator]() { return W.listItems(this).values(); }
  get [Symbol.toStringTag]() { return 'StyleSheetList'; }
}
C.makeStyleSheetList = (ids) => new StyleSheetList(ids);
C.sheetOfElement = (el) => sheetById(W.sheetOf(el));
let adopted = [];
C.adoptedSheets = () => adopted;
C.setAdoptedSheets = (v) => {
  const arr = Array.from(v);
  for (const s of arr) if (!(s instanceof CSSStyleSheet) || !s['%constructed']) throw domError('NotAllowedError', "Failed to set the 'adoptedStyleSheets' property on 'Document': Can't adopt non-constructed stylesheets.");
  adopted = arr;
  W.setAdopted(arr.map((s) => s['%id']));
};

// ---------------------------------------------------------------- CSS namespace, media queries

const CSS = {
  supports(a, b) { return arguments.length >= 2 ? W.cssSupports(String(a), String(b)) : W.cssSupports(String(a)); },
  escape(s) { return W.cssEscape(String(s)); },
  registerProperty() {},
  px: (v) => ({ value: v, unit: 'px', toString: () => v + 'px' }),
  number: (v) => ({ value: v, unit: 'number', toString: () => String(v) }),
  percent: (v) => ({ value: v, unit: 'percent', toString: () => v + '%' }),
  highlights: new Map(),
};

const mediaLists = [];
class MediaQueryList extends EventTarget {
  constructor(query) {
    super();
    const r = W.mediaMatches(query);
    this._media = r[1];
    this._matches = r[0];
    this.onchange = null;
    mediaLists.push(this);
  }
  get media() { return this._media; }
  get matches() { return this._matches; }
  addListener(fn) { if (fn) this.addEventListener('change', fn); }
  removeListener(fn) { if (fn) this.removeEventListener('change', fn); }
  _reevaluate() {
    const now = W.mediaMatches(this._media)[0];
    if (now !== this._matches) {
      this._matches = now;
      const ev = new MediaQueryListEvent('change', { media: this._media, matches: now });
      if (typeof this.onchange === 'function') { try { this.onchange.call(this, ev); } catch (e) { C.reportError(e); } }
      fire(this, ev);
    }
  }
  get [Symbol.toStringTag]() { return 'MediaQueryList'; }
}
class MediaQueryListEvent extends C.Event {
  constructor(type, init) { super(type, init); init = init || {}; this.media = init.media || ''; this.matches = !!init.matches; }
}
function matchMedia(query) { return new MediaQueryList(String(query)); }
C.reevaluateMedia = () => { for (const m of mediaLists) m._reevaluate(); };

Object.assign(C, { CSSStyleDeclaration, getComputedStyle, CSSRule, CSSStyleRule, CSSGroupingRule, CSSConditionRule, CSSMediaRule, CSSSupportsRule, CSSLayerBlockRule, CSSLayerStatementRule, CSSImportRule, CSSFontFaceRule, CSSPageRule, CSSNamespaceRule, CSSKeyframeRule, CSSKeyframesRule, CSSRuleList, MediaList, StyleSheet, CSSStyleSheet, StyleSheetList, CSS, MediaQueryList, MediaQueryListEvent, matchMedia });
})();
