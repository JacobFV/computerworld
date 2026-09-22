'use strict';
// ECMA-402 over the locale data in `data/cldr.json`, which is recorded from
// ICU by `tools/generate_cldr.js`. A pattern from that file is a string where
// `$x` stands for a field and everything else is literal text, so formatting is
// filling a pattern in and `formatToParts` is the same walk without joining.

let DATA = null;
function cldr() {
  if (DATA === null) DATA = binding.cldr();
  return DATA;
}

const LOCALES = [
  'en-US', 'en-GB', 'de-DE', 'fr-FR', 'es-ES', 'it-IT', 'pt-BR',
  'ja-JP', 'zh-CN', 'zh-TW', 'ko-KR', 'ru-RU', 'ar-EG', 'hi-IN',
];
// What a language falls back to when the exact tag is not one of ours.
const LANGUAGE_DEFAULT = {
  en: 'en-US', de: 'de-DE', fr: 'fr-FR', es: 'es-ES', it: 'it-IT', pt: 'pt-BR',
  ja: 'ja-JP', zh: 'zh-CN', ko: 'ko-KR', ru: 'ru-RU', ar: 'ar-EG', hi: 'hi-IN',
};
const REGION_ALIAS = {
  'en-AU': 'en-GB', 'en-NZ': 'en-GB', 'en-IE': 'en-GB', 'en-IN': 'en-GB',
  'en-ZA': 'en-GB', 'en-CA': 'en-US', 'de-AT': 'de-DE', 'de-CH': 'de-DE',
  'fr-CA': 'fr-FR', 'fr-BE': 'fr-FR', 'fr-CH': 'fr-FR', 'es-MX': 'es-ES',
  'es-AR': 'es-ES', 'es-419': 'es-ES', 'pt-PT': 'pt-BR', 'zh-TW': 'zh-TW',
  'zh-HK': 'zh-TW', 'zh-Hant': 'zh-TW', 'zh-Hans': 'zh-CN', 'ar-SA': 'ar-EG',
  'ar-AE': 'ar-EG', 'it-CH': 'it-IT',
};
const DEFAULT_LOCALE = 'en-US';

function canonicalTag(tag) {
  const s = String(tag).replace(/_/g, '-');
  const parts = s.split('-').filter((p) => p.length);
  if (!parts.length) return '';
  const out = [parts[0].toLowerCase()];
  for (const p of parts.slice(1)) {
    if (p.length === 4) out.push(p[0].toUpperCase() + p.slice(1).toLowerCase());
    else if (p.length === 2) out.push(p.toUpperCase());
    else out.push(p.toLowerCase());
  }
  return out.join('-');
}

function bestFit(tag) {
  const canon = canonicalTag(tag);
  if (!canon) return null;
  if (LOCALES.includes(canon)) return canon;
  if (REGION_ALIAS[canon]) return REGION_ALIAS[canon];
  const parts = canon.split('-');
  // Drop subtags from the right until something matches.
  for (let i = parts.length - 1; i >= 1; i--) {
    const shorter = parts.slice(0, i).join('-');
    if (LOCALES.includes(shorter)) return shorter;
    if (REGION_ALIAS[shorter]) return REGION_ALIAS[shorter];
  }
  return LANGUAGE_DEFAULT[parts[0]] || null;
}

function resolve(locales) {
  const list = locales === undefined ? [] : Array.isArray(locales) ? locales : [locales];
  for (const l of list) {
    const hit = bestFit(typeof l === 'object' && l !== null ? l.toString() : l);
    if (hit) return hit;
  }
  return DEFAULT_LOCALE;
}

function localeData(tag) {
  return cldr().locales[tag] || cldr().locales[DEFAULT_LOCALE];
}

function requireOptions(o) {
  if (o === undefined) return {};
  if (o === null) throw new TypeError('Cannot convert undefined or null to object');
  return Object(o);
}

// ---------------------------------------------------------------- patterns

/** Walks a pattern, calling `field(letter)` and `text(string)` in order. */
function walk(pattern, text, field) {
  let literal = '';
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i];
    if (c !== '$') {
      literal += c;
      continue;
    }
    const next = pattern[i + 1];
    i += 1;
    if (next === '$') {
      literal += '$';
      continue;
    }
    if (literal) {
      text(literal);
      literal = '';
    }
    field(next);
  }
  if (literal) text(literal);
}

// ---------------------------------------------------------------- numbers

const PLURAL = {
  // Enough of CLDR's plural rules for the locales the data covers.
  'en-US': cardinalEnglish, 'en-GB': cardinalEnglish, 'de-DE': cardinalEnglish,
  'it-IT': cardinalEnglish, 'es-ES': cardinalSpanish, 'pt-BR': cardinalPortuguese,
  'fr-FR': cardinalFrench, 'ru-RU': cardinalRussian, 'ar-EG': cardinalArabic,
  'hi-IN': cardinalHindi, 'ja-JP': cardinalOther, 'zh-CN': cardinalOther,
  'zh-TW': cardinalOther, 'ko-KR': cardinalOther,
};

/**
 * The operands CLDR's plural rules are written against. `visible` is the number
 * of fraction digits the formatted number shows, which is not always the number
 * the value has (`1.00` is plural in English, `1` is not).
 */
function decompose(n, visible) {
  const i = Math.floor(Math.abs(n));
  const s = String(Math.abs(n));
  const dot = s.indexOf('.');
  const frac = dot === -1 ? '' : s.slice(dot + 1);
  const v = visible === undefined ? frac.length : visible;
  return { n: Math.abs(n), i, v, f: frac === '' ? 0 : Number(frac) };
}

function cardinalEnglish(n, v) {
  const d = decompose(n, v);
  return d.i === 1 && d.v === 0 ? 'one' : 'other';
}
function cardinalSpanish(n, v) {
  const d = decompose(n, v);
  return d.n === 1 ? 'one' : 'other';
}
function cardinalPortuguese(n, v) {
  const d = decompose(n, v);
  return d.i === 0 || d.i === 1 ? 'one' : 'other';
}
function cardinalFrench(n, v) {
  const d = decompose(n, v);
  if (d.i === 0 || d.i === 1) return 'one';
  if (d.i !== 0 && d.i % 1000000 === 0 && d.v === 0) return 'many';
  return 'other';
}
function cardinalRussian(n, v) {
  const d = decompose(n, v);
  if (d.v !== 0) return 'other';
  const i10 = d.i % 10;
  const i100 = d.i % 100;
  if (i10 === 1 && i100 !== 11) return 'one';
  if (i10 >= 2 && i10 <= 4 && (i100 < 12 || i100 > 14)) return 'few';
  return 'many';
}
function cardinalArabic(n, v) {
  const d = decompose(n, v);
  if (d.n === 0) return 'zero';
  if (d.n === 1) return 'one';
  if (d.n === 2) return 'two';
  const m = d.n % 100;
  if (m >= 3 && m <= 10) return 'few';
  if (m >= 11 && m <= 99) return 'many';
  return 'other';
}
function cardinalHindi(n, v) {
  const d = decompose(n, v);
  return d.i === 0 || d.n === 1 ? 'one' : 'other';
}
function cardinalOther() {
  return 'other';
}

function ordinalCategory(locale, n) {
  const i = Math.floor(Math.abs(n));
  switch (locale) {
    case 'en-US':
    case 'en-GB': {
      const t = i % 10;
      const h = i % 100;
      if (t === 1 && h !== 11) return 'one';
      if (t === 2 && h !== 12) return 'two';
      if (t === 3 && h !== 13) return 'few';
      return 'other';
    }
    case 'fr-FR':
      return i === 1 ? 'one' : 'other';
    case 'it-IT':
      return [11, 8, 80, 800].includes(i) ? 'many' : 'other';
    case 'hi-IN':
      if (i === 1) return 'one';
      if (i === 2 || i === 3) return 'two';
      if (i === 4) return 'few';
      if (i === 6) return 'many';
      return 'other';
    default:
      return 'other';
  }
}

const NUMBER_DEFAULTS = {
  decimal: [0, 3], percent: [0, 0], currency: [2, 2], unit: [0, 3],
};

/** Rewrites ASCII digits into the locale's numbering system. */
function localDigits(text, digits) {
  if (!digits || digits === '0123456789') return text;
  let out = '';
  for (const c of text) {
    const i = c.charCodeAt(0) - 48;
    out += i >= 0 && i <= 9 ? digits[i] : c;
  }
  return out;
}

/** Rounds a decimal string half away from zero, as ICU does. */
function roundDecimal(value, places) {
  // 20 significant digits is more than a double carries, so the string is the
  // number: rounding it is exact where `toFixed` would round the binary value.
  // The shortest representation is the number as the language means it, which
  // is what ICU rounds; rounding the binary expansion would go the other way
  // for values like 1.2345.
  let s = String(Math.abs(value));
  if (s.includes('e')) return Math.abs(value).toFixed(Math.min(20, Math.max(0, places)));
  let [int, frac = ''] = s.split('.');
  if (frac.length <= places) return `${int}${frac ? `.${frac}` : ''}`;
  const keep = frac.slice(0, places);
  const next = frac.charCodeAt(places) - 48;
  let digits = int + keep;
  if (next >= 5) {
    // Carry the increment through the digits.
    let i = digits.length - 1;
    const chars = digits.split('');
    for (;;) {
      if (i < 0) {
        chars.unshift('1');
        break;
      }
      if (chars[i] === '9') {
        chars[i] = '0';
        i -= 1;
      } else {
        chars[i] = String.fromCharCode(chars[i].charCodeAt(0) + 1);
        break;
      }
    }
    digits = chars.join('');
  }
  const cut = digits.length - places;
  const head = digits.slice(0, cut) || '0';
  const tail = digits.slice(cut);
  return places > 0 ? `${head}.${tail}` : head;
}

function roundToString(value, min, max) {
  if (!isFinite(value)) return String(value);
  // `toFixed` gives up on exponential notation above 1e21, where the value is
  // an integer anyway.
  if (Math.abs(value) >= 1e21) {
    const whole = BigInt(Math.round(Math.abs(value))).toString();
    return min > 0 ? `${whole}.${'0'.repeat(min)}` : whole;
  }
  let s = roundDecimal(value, Math.min(20, max));
  if (s.includes('.')) {
    s = s.replace(/0+$/, '');
    const dot = s.indexOf('.');
    const have = s.length - dot - 1;
    if (have < min) s += '0'.repeat(min - have);
    else if (s.endsWith('.')) s = s.slice(0, -1);
  }
  if (min > 0 && !s.includes('.')) s = `${s}.${'0'.repeat(min)}`;
  return s;
}

function significantToString(value, min, max) {
  if (value === 0) return (0).toFixed(Math.max(0, min - 1));
  let s = Math.abs(value).toPrecision(max);
  if (s.includes('e')) s = String(Number(s));
  if (s.includes('.')) {
    s = s.replace(/0+$/, '').replace(/\.$/, '');
    const digits = s.replace(/[^0-9]/g, '').replace(/^0+/, '').length;
    if (digits < min) s = Math.abs(value).toPrecision(min);
  }
  return s;
}

function group(digits, sizes, minGroupingDigits, symbol, useGrouping) {
  // A locale with `minGroupingDigits: 2` leaves four-digit numbers ungrouped.
  if (!useGrouping || digits.length < sizes[0] + minGroupingDigits) return [digits];
  const out = [];
  let rest = digits;
  let size = sizes[0];
  while (rest.length > size) {
    out.unshift(rest.slice(rest.length - size));
    rest = rest.slice(0, rest.length - size);
    size = sizes[1];
  }
  out.unshift(rest);
  const joined = [];
  out.forEach((part, i) => {
    if (i) joined.push({ type: 'group', value: symbol });
    joined.push({ type: 'integer', value: part });
  });
  return joined;
}

class NumberFormat {
  constructor(locales, options) {
    const o = requireOptions(options);
    const tag = resolve(locales);
    const d = localeData(tag);
    const style = o.style === undefined ? 'decimal' : String(o.style);
    if (!['decimal', 'percent', 'currency', 'unit'].includes(style)) {
      throw new RangeError(`Value ${style} out of range for Intl.NumberFormat options property style`);
    }
    if (style === 'currency' && o.currency === undefined) {
      throw new TypeError('Currency code is required with currency style.');
    }
    if (style === 'unit' && o.unit === undefined) {
      throw new TypeError('Invalid unit argument for Intl.NumberFormat()');
    }
    const currency = o.currency === undefined ? undefined : String(o.currency).toUpperCase();
    if (currency !== undefined && !/^[A-Z]{3}$/.test(currency)) {
      throw new RangeError(`Invalid currency code : ${o.currency}`);
    }
    const info = currency ? d.currencies[currency] : null;
    const defaults = NUMBER_DEFAULTS[style];
    const currencyDigits = info ? info[3] : 2;
    this._d = d;
    this._locale = tag;
    this._style = style;
    this._currency = currency;
    this._currencyDisplay = o.currencyDisplay === undefined ? 'symbol' : String(o.currencyDisplay);
    this._unit = o.unit === undefined ? undefined : String(o.unit);
    this._unitDisplay = o.unitDisplay === undefined ? 'short' : String(o.unitDisplay);
    this._notation = o.notation === undefined ? 'standard' : String(o.notation);
    this._compactDisplay = o.compactDisplay === undefined ? 'short' : String(o.compactDisplay);
    this._signDisplay = o.signDisplay === undefined ? 'auto' : String(o.signDisplay);
    this._useGrouping = o.useGrouping === undefined ? true : o.useGrouping;
    this._minInt = o.minimumIntegerDigits === undefined ? 1 : Number(o.minimumIntegerDigits);
    const minFraction = style === 'currency' ? currencyDigits : defaults[0];
    const maxFraction = style === 'currency' ? currencyDigits : defaults[1];
    this._minFrac = o.minimumFractionDigits === undefined
      ? minFraction
      : Number(o.minimumFractionDigits);
    this._maxFrac = o.maximumFractionDigits === undefined
      ? Math.max(this._minFrac, maxFraction)
      : Number(o.maximumFractionDigits);
    if (this._maxFrac < this._minFrac) {
      throw new RangeError('maximumFractionDigits value is out of range.');
    }
    this._minSig = o.minimumSignificantDigits === undefined
      ? undefined
      : Number(o.minimumSignificantDigits);
    this._maxSig = o.maximumSignificantDigits === undefined
      ? (this._minSig === undefined ? undefined : 21)
      : Number(o.maximumSignificantDigits);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.NumberFormat';
  }

  resolvedOptions() {
    const out = {
      locale: this._locale,
      numberingSystem: 'latn',
      style: this._style,
      minimumIntegerDigits: this._minInt,
      minimumFractionDigits: this._minFrac,
      maximumFractionDigits: this._maxFrac,
      useGrouping: this._useGrouping === true ? 'auto' : this._useGrouping,
      notation: this._notation,
      signDisplay: this._signDisplay,
      roundingIncrement: 1,
      roundingMode: 'halfExpand',
      roundingPriority: 'auto',
      trailingZeroDisplay: 'auto',
    };
    if (this._currency) {
      out.currency = this._currency;
      out.currencyDisplay = this._currencyDisplay;
      out.currencySign = 'standard';
    }
    if (this._unit) {
      out.unit = this._unit;
      out.unitDisplay = this._unitDisplay;
    }
    if (this._minSig !== undefined) out.minimumSignificantDigits = this._minSig;
    if (this._maxSig !== undefined) out.maximumSignificantDigits = this._maxSig;
    if (this._notation === 'compact') out.compactDisplay = this._compactDisplay;
    return out;
  }

  /** The pattern a value takes, and the value scaled to it. */
  _shape(value) {
    const d = this._d;
    const negative = value < 0 || Object.is(value, -0);
    const pick = (pair) => pair[negative ? 1 : 0];
    if (this._style === 'percent') return { pattern: pick(d.number.percent), scale: 100 };
    if (this._style === 'currency') {
      const key = this._currencyDisplay === 'code'
        ? 'code'
        : this._currencyDisplay === 'name' ? 'name' : 'symbol';
      return { pattern: pick(d.currencyShapes[key]), scale: 1 };
    }
    if (this._style === 'unit') {
      const unit = d.units[this._unit];
      if (!unit) {
        return { pattern: `$n ${this._unit}`, scale: 1, plain: true };
      }
      const forms = unit[this._unitDisplay] || unit.short;
      const one = PLURAL[this._locale](Math.abs(value)) === 'one';
      const pattern = forms[one ? 0 : 1];
      return { pattern: negative ? `${d.number.symbols.minus}${pattern}` : pattern, scale: 1 };
    }
    if (this._notation === 'scientific' || this._notation === 'engineering') {
      return { pattern: pick(d.number.scientific), scale: 1 };
    }
    return { pattern: pick(d.number.decimal), scale: 1 };
  }

  formatToParts(input) {
    const value = typeof input === 'bigint' ? Number(input) : Number(input);
    const d = this._d;
    const symbols = d.number.symbols;
    const negative = value < 0 || Object.is(value, -0);
    const parts = [];
    let currencyText = null;
    if (Number.isNaN(value)) return [{ type: 'nan', value: symbols.nan }];
    const shape = this._shape(value);
    let magnitude = Math.abs(value) * shape.scale;
    let compactSuffix = null;
    let exponent = null;
    if (this._notation === 'compact' && isFinite(magnitude) && magnitude >= 1000) {
      const table = d.number.compact[this._compactDisplay];
      let e = Math.floor(Math.log10(magnitude));
      while (e > 14) e -= 1;
      const entry = table[String(e)];
      if (entry) {
        // The unit the locale counts in: a thousand, or ten thousand where the
        // language groups that way.
        magnitude /= 10 ** entry[2];
        // A compact number keeps one fraction digit while its integer part is
        // a single digit, and none after that.
        magnitude = Number(magnitude.toFixed(magnitude >= 10 ? 0 : 1));
        // Exactly one of a unit can have a form of its own (`mille`).
        compactSuffix = magnitude === 1 ? entry[1] : entry[0];
      }
    }
    if (this._notation === 'scientific' || this._notation === 'engineering') {
      if (magnitude !== 0 && isFinite(magnitude)) {
        let e = Math.floor(Math.log10(magnitude));
        if (this._notation === 'engineering') e = Math.floor(e / 3) * 3;
        magnitude /= 10 ** e;
        exponent = e;
      } else {
        exponent = 0;
      }
    }
    let digits;
    if (this._minSig !== undefined || this._maxSig !== undefined) {
      digits = significantToString(magnitude, this._minSig || 1, this._maxSig || 21);
    } else if (this._notation === 'compact') {
      digits = roundToString(magnitude, 0, magnitude >= 10 ? 0 : 1);
    } else {
      digits = roundToString(magnitude, this._minFrac, this._maxFrac);
    }
    if (!isFinite(magnitude)) digits = symbols.infinity;
    let [int, frac] = digits.split('.');
    while (int.length < this._minInt) int = `0${int}`;
    const emitNumber = () => {
      if (!isFinite(magnitude)) {
        parts.push({ type: 'infinity', value: symbols.infinity });
        return;
      }
      const grouped = group(
        int,
        d.number.grouping,
        d.number.minGroupingDigits || 1,
        symbols.group,
        this._useGrouping !== false && this._useGrouping !== 'false'
          && this._notation !== 'scientific' && this._notation !== 'compact',
      );
      for (const g of grouped) {
        if (typeof g === 'string') parts.push({ type: 'integer', value: localDigits(g, symbols.digits) });
        else if (g.type === 'integer') parts.push({ type: 'integer', value: localDigits(g.value, symbols.digits) });
        else parts.push(g);
      }
      if (frac) {
        parts.push({ type: 'decimal', value: symbols.decimal });
        parts.push({ type: 'fraction', value: localDigits(frac, symbols.digits) });
      }
    };
    const pattern = compactSuffix
      ? (negative ? `${symbols.minus}${compactSuffix}` : compactSuffix)
      : shape.pattern;
    const sign = this._signDisplay;
    const wantsPlus = !negative && value !== 0
      && (sign === 'always' || sign === 'exceptZero');
    const dropsSign = negative && (sign === 'never'
      || (sign === 'exceptZero' && value === 0));
    walk(pattern, (text) => {
      parts.push({ type: 'literal', value: text });
    }, (f) => {
      switch (f) {
        case 'n':
          if (wantsPlus) parts.push({ type: 'plusSign', value: symbols.plus });
          emitNumber();
          break;
        case '-':
          if (!dropsSign) parts.push({ type: 'minusSign', value: symbols.minus });
          break;
        case '+':
          parts.push({ type: 'plusSign', value: symbols.plus });
          break;
        case '%':
          parts.push({ type: 'percentSign', value: symbols.percent });
          break;
        case 'c': {
          const info = d.currencies[this._currency];
          let text = this._currency;
          if (info) {
            if (this._currencyDisplay === 'name') text = info[2];
            else if (this._currencyDisplay === 'narrowSymbol') text = info[1];
            else if (this._currencyDisplay !== 'code') text = info[0];
          }
          currencyText = text;
          parts.push({ type: 'currency', value: text });
          break;
        }
        case 'e':
          parts.push({ type: 'exponentSeparator', value: symbols.exponential });
          if (exponent !== null && exponent < 0) {
            parts.push({ type: 'exponentMinusSign', value: symbols.minus });
          }
          parts.push({
            type: 'exponentInteger',
            value: localDigits(String(Math.abs(exponent || 0)), symbols.digits),
          });
          break;
        case 'x':
          break;
        default:
          break;
      }
    });
    if (this._style === 'currency' && currencyText !== null
        && this._currencyDisplay !== 'name') {
      // CLDR's currency spacing: a symbol that ends (or starts) with a letter
      // is held off the digits by a no-break space.
      // A letter for the purpose of the rule: a cased one, or a letter of a
      // script that has no case (Arabic and Hebrew currency symbols).
      const alpha = (c) => c !== undefined
        && (c.toLowerCase() !== c.toUpperCase() || /[\u0590-\u08ff]/.test(c));
      for (let i = 0; i < parts.length; i++) {
        if (parts[i].type !== 'currency') continue;
        const before = parts[i - 1];
        const after = parts[i + 1];
        if (after && ['integer', 'minusSign', 'plusSign'].includes(after.type)
            && alpha(currencyText[currencyText.length - 1])) {
          parts.splice(i + 1, 0, { type: 'literal', value: '\u00a0' });
        } else if (before && ['integer', 'fraction'].includes(before.type)
            && alpha(currencyText[0])) {
          parts.splice(i, 0, { type: 'literal', value: '\u00a0' });
          i += 1;
        }
      }
    }
    if (this._style === 'currency' && this._currencyDisplay === 'name') {
      // The name form carries the plural category of the value.
      const info = d.currencies[this._currency];
      const shownFraction = (frac || '').length;
      if (info && PLURAL[this._locale](Math.abs(value), shownFraction) !== 'one') {
        for (const p of parts) {
          if (p.type === 'currency' && p.value === info[2]) p.value = pluralCurrencyName(info[2]);
        }
      }
    }
    return parts;
  }

  format(value) {
    return this.formatToParts(value).map((p) => p.value).join('');
  }

  formatRange(a, b) {
    return `${this.format(a)} – ${this.format(b)}`;
  }

  formatRangeToParts(a, b) {
    const left = this.formatToParts(a).map((p) => ({ ...p, source: 'startRange' }));
    const right = this.formatToParts(b).map((p) => ({ ...p, source: 'endRange' }));
    return [...left, { type: 'literal', value: ' – ', source: 'shared' }, ...right];
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

/** English-style plural of a currency's display name (`US dollars`). */
function pluralCurrencyName(name) {
  if (/[a-z]$/.test(name) && !/s$/.test(name)) return `${name}s`;
  return name;
}

// ------------------------------------------------------------------ dates

const FIELD_ORDER = ['era', 'year', 'month', 'day', 'weekday', 'hour', 'minute', 'second'];

function skeletonKey(o, hourCycle) {
  let key = '';
  if (o.era) key += 'G';
  if (o.year) key += 'y';
  if (o.month) {
    key += { numeric: 'M', '2-digit': 'MM', short: 'MMM', long: 'MMMM', narrow: 'MMMMM' }[o.month];
  }
  if (o.day) key += 'd';
  if (o.weekday) key += { short: 'E', long: 'EEEE', narrow: 'EEEEE' }[o.weekday];
  const h = hourCycle === 'h12' || hourCycle === 'h11' ? 'h' : 'H';
  let time = '';
  // A two-digit hour can have a pattern of its own (`15:34:56` where a
  // one-digit hour reads `15시 34분 56초`).
  if (o.hour === '2-digit' && o.minute) {
    time = o.second ? `${h}${h}mmss` : `${h}${h}mm`;
    if (o.timeZoneName) time += o.timeZoneName === 'long' ? 'vvvv' : 'v';
    return { date: key, time };
  }
  if (o.hour) time += h;
  if (o.minute) time += 'm';
  if (o.second) time += 's';
  if (o.fractionalSecondDigits) time += 'S';
  if (o.timeZoneName) time += o.timeZoneName === 'long' ? 'vvvv' : 'v';
  return { date: key, time };
}

function pad(n, width) {
  let s = String(Math.abs(n));
  while (s.length < width) s = `0${s}`;
  return n < 0 ? `-${s}` : s;
}

/** Civil fields of an instant in a zone. */
function civil(epochMs, zone) {
  const info = zone === 'UTC' ? [0, 'UTC', false] : binding.tzOffset(zone, epochMs);
  const offset = info ? info[0] : 0;
  const local = epochMs + offset * 1000;
  const d = new Date(local);
  return {
    year: d.getUTCFullYear(),
    month: d.getUTCMonth() + 1,
    day: d.getUTCDate(),
    weekday: d.getUTCDay(),
    hour: d.getUTCHours(),
    minute: d.getUTCMinutes(),
    second: d.getUTCSeconds(),
    millisecond: d.getUTCMilliseconds(),
    offset,
    abbreviation: info ? info[1] : 'UTC',
    isDst: info ? info[2] : false,
  };
}

function offsetName(offset, long) {
  if (offset === 0) return long ? 'Coordinated Universal Time' : 'UTC';
  const sign = offset < 0 ? '-' : '+';
  const total = Math.abs(offset);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  if (long) return `GMT${sign}${pad(h, 2)}:${pad(m, 2)}`;
  return m ? `GMT${sign}${h}:${pad(m, 2)}` : `GMT${sign}${h}`;
}

class DateTimeFormat {
  constructor(locales, options) {
    const o = requireOptions(options);
    const tag = resolve(locales);
    const d = localeData(tag);
    this._locale = tag;
    this._d = d;
    this._tz = o.timeZone === undefined ? 'UTC' : String(o.timeZone);
    if (o.timeZone !== undefined && binding.tzOffset(this._tz, 0) === null) {
      throw new RangeError(`Invalid time zone specified: ${o.timeZone}`);
    }
    const has = ['weekday', 'era', 'year', 'month', 'day', 'hour', 'minute', 'second',
      'timeZoneName', 'fractionalSecondDigits', 'dayPeriod']
      .some((k) => o[k] !== undefined);
    this._dateStyle = o.dateStyle === undefined ? undefined : String(o.dateStyle);
    this._timeStyle = o.timeStyle === undefined ? undefined : String(o.timeStyle);
    if ((this._dateStyle || this._timeStyle) && has) {
      throw new TypeError("Can't set option dateStyle or timeStyle with other date-time component options");
    }
    this._opts = {};
    for (const k of ['weekday', 'era', 'year', 'month', 'day', 'hour', 'minute', 'second',
      'timeZoneName', 'dayPeriod']) {
      if (o[k] !== undefined) this._opts[k] = String(o[k]);
    }
    if (o.fractionalSecondDigits !== undefined) {
      this._opts.fractionalSecondDigits = Number(o.fractionalSecondDigits);
    }
    if (!has && !this._dateStyle && !this._timeStyle) {
      this._opts = { year: 'numeric', month: 'numeric', day: 'numeric' };
    }
    const cycle = d.hourCycle;
    if (o.hour12 !== undefined) this._hourCycle = o.hour12 ? 'h12' : 'h23';
    else if (o.hourCycle !== undefined) this._hourCycle = String(o.hourCycle);
    else this._hourCycle = cycle;
  }

  get [Symbol.toStringTag]() {
    return 'Intl.DateTimeFormat';
  }

  resolvedOptions() {
    const out = {
      locale: this._locale,
      calendar: 'gregory',
      numberingSystem: 'latn',
      timeZone: this._tz,
    };
    if (this._dateStyle) out.dateStyle = this._dateStyle;
    if (this._timeStyle) out.timeStyle = this._timeStyle;
    Object.assign(out, this._opts);
    if (this._opts.hour) {
      out.hourCycle = this._hourCycle;
      out.hour12 = this._hourCycle === 'h11' || this._hourCycle === 'h12';
    }
    return out;
  }

  _pattern() {
    const d = this._d;
    if (this._dateStyle && this._timeStyle) {
      return d.dateTimeStyles[this._dateStyle][this._timeStyle];
    }
    if (this._dateStyle) return d.dateStyles[this._dateStyle];
    if (this._timeStyle) return d.timeStyles[this._timeStyle];
    const { date, time } = skeletonKey(this._opts, this._hourCycle);
    const datePattern = date ? d.patterns[date] : '';
    const timePattern = time ? d.patterns[time] : '';
    if (datePattern && timePattern) {
      // The locale's own way of joining a date to a time, taken from the
      // medium styles it already has.
      const glue = d.dateTimeStyles.medium.medium;
      const dateOnly = d.dateStyles.medium;
      const timeOnly = d.timeStyles.medium;
      const joiner = glue.replace(dateOnly, '').replace(timeOnly, '');
      if (joiner.includes('') && joiner.includes('')) {
        return joiner.replace('', datePattern).replace('', timePattern);
      }
      return `${datePattern} ${timePattern}`;
    }
    if (datePattern || timePattern) return datePattern || timePattern;
    // Nothing recorded for this combination: fall back to the widest date
    // pattern that has the fields asked for.
    return d.patterns.yMd;
  }

  /** The widths every field is formatted with: a style resolves to its own. */
  _widths() {
    const d = this._d;
    if (this._dateStyle && this._timeStyle) {
      return d.styleOptions.combined[this._dateStyle][this._timeStyle];
    }
    if (this._dateStyle) return d.styleOptions.date[this._dateStyle];
    if (this._timeStyle) return d.styleOptions.time[this._timeStyle];
    return this._opts;
  }

  formatToParts(input) {
    const ms = input === undefined ? Date.now() : Number(input instanceof Date ? input.getTime() : input);
    if (!isFinite(ms)) throw new RangeError('Invalid time value');
    const d = this._d;
    const c = civil(ms, this._tz);
    const o = this._widths();
    const parts = [];
    let seenTimeField = false;
    const digits = d.number.symbols.digits;
    const num = (value) => localDigits(String(value), digits);
    const two = (k, value) => num(o[k] === '2-digit' ? pad(value, 2) : String(value));
    walk(this._pattern(), (text) => parts.push({ type: 'literal', value: text }), (f) => {
      switch (f) {
        case 'G': {
          const width = o.era === 'long' ? 'long' : o.era === 'narrow' ? 'narrow' : 'short';
          parts.push({ type: 'era', value: d.eras[width][c.year > 0 ? 1 : 0] });
          break;
        }
        case 'y': {
          const y = c.year > 0 ? c.year : 1 - c.year;
          parts.push({
            type: 'year',
            value: num(o.year === '2-digit' ? pad(y % 100, 2) : String(y)),
          });
          break;
        }
        case 'M':
        case 'L': {
          const w = o.month;
          let value;
          if (w === 'long' || w === 'short' || w === 'narrow') {
            value = (f === 'L' ? d.monthsAlone : d.months)[w][c.month - 1];
          } else value = two('month', c.month);
          parts.push({ type: 'month', value });
          break;
        }
        case 'd':
          parts.push({ type: 'day', value: two('day', c.day) });
          break;
        case 'E':
        case 'C': {
          const w = o.weekday === 'long' || o.weekday === 'narrow' ? o.weekday : 'short';
          const names = f === 'C' ? d.weekdaysAlone : d.weekdays;
          parts.push({ type: 'weekday', value: names[w][c.weekday] });
          break;
        }
        case 'h': {
          let h = c.hour;
          const cycle = this._hourCycle;
          if (cycle === 'h12') h = h % 12 === 0 ? 12 : h % 12;
          else if (cycle === 'h11') h %= 12;
          else if (cycle === 'h24') h = h === 0 ? 24 : h;
          parts.push({ type: 'hour', value: two('hour', h) });
          seenTimeField = true;
          break;
        }
        case 'm':
          // A minute after an hour is always two digits, as in every locale's
          // time pattern; on its own it takes the width asked for.
          parts.push({
            type: 'minute',
            value: num(seenTimeField || o.minute === '2-digit' ? pad(c.minute, 2) : c.minute),
          });
          seenTimeField = true;
          break;
        case 's':
          parts.push({
            type: 'second',
            value: num(seenTimeField || o.second === '2-digit' ? pad(c.second, 2) : c.second),
          });
          seenTimeField = true;
          break;
        case 'S': {
          const value = pad(c.millisecond, 3).slice(0, o.fractionalSecondDigits || 3);
          parts.push({ type: 'fractionalSecond', value: num(value) });
          break;
        }
        case 'a':
          parts.push({ type: 'dayPeriod', value: d.dayPeriods[c.hour < 12 ? 0 : 1] });
          break;
        case 'v': {
          const long = o.timeZoneName === 'long';
          const named = d.zoneNames[this._tz];
          const entry = named ? named[String(c.offset)] : undefined;
          const value = entry ? entry[long ? 1 : 0] : offsetName(c.offset, long);
          parts.push({ type: 'timeZoneName', value });
          break;
        }
        default:
          break;
      }
    });
    return parts;
  }

  format(input) {
    // V8 hands `format` a plain space where the pattern has a narrow no-break
    // one, while `formatToParts` keeps it; both are reproduced.
    return this.formatToParts(input).map((p) => p.value).join('').split('\u202f').join(' ');
  }

  formatRange(a, b) {
    return `${this.format(a)} – ${this.format(b)}`;
  }

  formatRangeToParts(a, b) {
    const left = this.formatToParts(a).map((p) => ({ ...p, source: 'startRange' }));
    const right = this.formatToParts(b).map((p) => ({ ...p, source: 'endRange' }));
    return [...left, { type: 'literal', value: ' – ', source: 'shared' }, ...right];
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

// ------------------------------------------------------------ other classes

class PluralRules {
  constructor(locales, options) {
    const o = requireOptions(options);
    this._locale = resolve(locales);
    this._type = o.type === undefined ? 'cardinal' : String(o.type);
    this._minFrac = o.minimumFractionDigits === undefined ? 0 : Number(o.minimumFractionDigits);
    this._maxFrac = o.maximumFractionDigits === undefined ? 3 : Number(o.maximumFractionDigits);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.PluralRules';
  }

  select(value) {
    const n = Number(value);
    if (!isFinite(n)) return 'other';
    if (this._type === 'ordinal') return ordinalCategory(this._locale, n);
    const rounded = Number(roundToString(n, this._minFrac, this._maxFrac));
    return PLURAL[this._locale](rounded);
  }

  selectRange(a, b) {
    return this.select(b);
  }

  resolvedOptions() {
    const categories = new Set();
    for (const n of [0, 1, 2, 3, 5, 11, 21, 100, 1000000]) categories.add(this.select(n));
    return {
      locale: this._locale,
      type: this._type,
      minimumIntegerDigits: 1,
      minimumFractionDigits: this._minFrac,
      maximumFractionDigits: this._maxFrac,
      pluralCategories: [...categories].sort(),
      roundingMode: 'halfExpand',
    };
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

const RELATIVE_UNITS = ['year', 'quarter', 'month', 'week', 'day', 'hour', 'minute', 'second'];

class RelativeTimeFormat {
  constructor(locales, options) {
    const o = requireOptions(options);
    this._locale = resolve(locales);
    this._d = localeData(this._locale);
    this._numeric = o.numeric === undefined ? 'always' : String(o.numeric);
    this._style = o.style === undefined ? 'long' : String(o.style);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.RelativeTimeFormat';
  }

  formatToParts(value, unit) {
    const n = Number(value);
    let u = String(unit);
    if (u.endsWith('s') && RELATIVE_UNITS.includes(u.slice(0, -1))) u = u.slice(0, -1);
    if (!RELATIVE_UNITS.includes(u)) {
      throw new RangeError(`Invalid unit argument for format() '${unit}'`);
    }
    const table = this._d.relative[u][this._numeric === 'auto' ? 'auto' : 'always'];
    // The sample whose plural category and sign match.
    const key = pickRelativeSample(this._locale, table, n, this._numeric === 'auto');
    const pattern = table[key];
    const parts = [];
    const nf = new NumberFormat(this._locale, { maximumFractionDigits: 3 });
    walk(pattern, (text) => parts.push({ type: 'literal', value: text }), (f) => {
      if (f !== 'n') return;
      for (const p of nf.formatToParts(Math.abs(n))) {
        parts.push({ type: p.type, value: p.value, unit: u });
      }
    });
    return parts;
  }

  format(value, unit) {
    return this.formatToParts(value, unit).map((p) => p.value).join('');
  }

  resolvedOptions() {
    return {
      locale: this._locale,
      style: this._style,
      numeric: this._numeric,
      numberingSystem: 'latn',
    };
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

/** The recorded sample whose sign and plural category match `n`. */
function pickRelativeSample(locale, table, n, auto) {
  if (table[String(n)] !== undefined && n >= -2 && n <= 2) return String(n);
  const category = PLURAL[locale](Math.abs(n));
  const sign = n < 0 ? -1 : 1;
  let fallback = null;
  for (const key of Object.keys(table)) {
    const k = Number(key);
    if (k === 0 || (k < 0 ? -1 : 1) !== sign) continue;
    // In the `auto` table the small values are words (`yesterday`), which only
    // stand for themselves.
    if (auto && Math.abs(k) <= 2) continue;
    if (PLURAL[locale](Math.abs(k)) === category) return key;
    if (fallback === null) fallback = key;
  }
  return fallback === null ? String(sign * 5) : fallback;
}

class ListFormat {
  constructor(locales, options) {
    const o = requireOptions(options);
    this._locale = resolve(locales);
    this._d = localeData(this._locale);
    this._type = o.type === undefined ? 'conjunction' : String(o.type);
    this._style = o.style === undefined ? 'long' : String(o.style);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.ListFormat';
  }

  formatToParts(list) {
    const items = Array.from(list === undefined ? [] : list).map(String);
    const table = (this._d.list[this._type] || this._d.list.conjunction)[this._style]
      || this._d.list[this._type].long;
    const out = [];
    if (!items.length) return out;
    if (items.length === 1) return [{ type: 'element', value: items[0] }];
    const literals = items.length === 2 ? table.two : table.three;
    // The recorded literals of a three-item list: [before, between, before-last, after].
    const between = literals.filter((l) => l !== null);
    if (items.length === 2) {
      out.push({ type: 'element', value: items[0] });
      if (between[0]) out.push({ type: 'literal', value: between[0] });
      out.push({ type: 'element', value: items[1] });
      return out;
    }
    const middle = between[0];
    const last = between[between.length - 1];
    items.forEach((item, i) => {
      if (i) out.push({ type: 'literal', value: i === items.length - 1 ? last : middle });
      out.push({ type: 'element', value: item });
    });
    return out;
  }

  format(list) {
    return this.formatToParts(list).map((p) => p.value).join('');
  }

  resolvedOptions() {
    return { locale: this._locale, type: this._type, style: this._style };
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

class DisplayNames {
  constructor(locales, options) {
    const o = requireOptions(options);
    if (options === undefined) throw new TypeError('Intl.DisplayNames constructor requires options');
    if (o.type === undefined) throw new TypeError('type must be provided');
    this._locale = resolve(locales);
    this._d = localeData(this._locale);
    this._type = String(o.type);
    this._fallback = o.fallback === undefined ? 'code' : String(o.fallback);
    this._style = o.style === undefined ? 'long' : String(o.style);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.DisplayNames';
  }

  of(code) {
    const c = String(code);
    const table = this._d.displayNames[this._type === 'calendar' ? 'language' : this._type];
    const key = this._type === 'region' ? c.toUpperCase()
      : this._type === 'currency' ? c.toUpperCase()
        : this._type === 'script' ? c[0].toUpperCase() + c.slice(1).toLowerCase()
          : c.toLowerCase();
    const hit = table ? table[key] : undefined;
    if (hit !== undefined) return hit;
    return this._fallback === 'none' ? undefined : c;
  }

  resolvedOptions() {
    return {
      locale: this._locale,
      style: this._style,
      type: this._type,
      fallback: this._fallback,
    };
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

// Collation: the locales here sort Latin text the way their CLDR tailoring
// does for the letters that differ from the root order.
const COLLATION = {
  'de-DE': {}, 'en-US': {}, 'en-GB': {}, 'fr-FR': {}, 'it-IT': {}, 'pt-BR': {},
  'es-ES': { ñ: 'n' },
  'ru-RU': {}, 'ja-JP': {}, 'zh-CN': {}, 'zh-TW': {}, 'ko-KR': {}, 'ar-EG': {}, 'hi-IN': {},
};

const ACCENTS = {
  á: 'a', à: 'a', â: 'a', ä: 'a', ã: 'a', å: 'a', ā: 'a',
  é: 'e', è: 'e', ê: 'e', ë: 'e', ē: 'e',
  í: 'i', ì: 'i', î: 'i', ï: 'i', ī: 'i',
  ó: 'o', ò: 'o', ô: 'o', ö: 'o', õ: 'o', ō: 'o', ø: 'o',
  ú: 'u', ù: 'u', û: 'u', ü: 'u', ū: 'u',
  ñ: 'n', ç: 'c', ß: 'ss', æ: 'ae', œ: 'oe', ý: 'y', ÿ: 'y',
};

function foldAccents(s) {
  let out = '';
  for (const ch of s) out += ACCENTS[ch.toLowerCase()] === undefined
    ? ch
    : (ch === ch.toLowerCase() ? ACCENTS[ch] : ACCENTS[ch.toLowerCase()].toUpperCase());
  return out;
}

class Collator {
  constructor(locales, options) {
    const o = requireOptions(options);
    this._locale = resolve(locales);
    this._usage = o.usage === undefined ? 'sort' : String(o.usage);
    this._sensitivity = o.sensitivity === undefined ? 'variant' : String(o.sensitivity);
    this._numeric = !!o.numeric;
    this._caseFirst = o.caseFirst === undefined ? 'false' : String(o.caseFirst);
    this._ignorePunctuation = !!o.ignorePunctuation;
    const self = this;
    this.compare = (a, b) => self._compare(String(a), String(b));
  }

  get [Symbol.toStringTag]() {
    return 'Intl.Collator';
  }

  _key(s) {
    let out = s;
    const tailoring = COLLATION[this._locale] || {};
    for (const [from, to] of Object.entries(tailoring)) out = out.split(from).join(to);
    if (this._ignorePunctuation) out = out.replace(/[\s\-_.,;:!?'"()[\]]/g, '');
    if (this._sensitivity === 'base' || this._sensitivity === 'case') out = foldAccents(out);
    if (this._sensitivity === 'base' || this._sensitivity === 'accent') out = out.toLowerCase();
    return out;
  }

  _compare(a, b) {
    if (this._numeric) {
      const re = /(\d+)|(\D+)/g;
      const pa = a.match(re) || [];
      const pb = b.match(re) || [];
      for (let i = 0; i < Math.min(pa.length, pb.length); i++) {
        const x = pa[i];
        const y = pb[i];
        if (/^\d/.test(x) && /^\d/.test(y)) {
          if (Number(x) !== Number(y)) return Number(x) < Number(y) ? -1 : 1;
        } else {
          const c = this._plain(x, y);
          if (c) return c;
        }
      }
      return pa.length === pb.length ? 0 : pa.length < pb.length ? -1 : 1;
    }
    return this._plain(a, b);
  }

  _plain(a, b) {
    const ka = this._key(a);
    const kb = this._key(b);
    // Compare letter by letter with accents as a lower-priority difference,
    // which is what a CLDR collation does at the secondary level.
    const fa = foldAccents(ka).toLowerCase();
    const fb = foldAccents(kb).toLowerCase();
    if (fa !== fb) return fa < fb ? -1 : 1;
    if (this._sensitivity === 'base') return 0;
    const aa = ka.toLowerCase();
    const bb = kb.toLowerCase();
    if (aa !== bb) return aa < bb ? -1 : 1;
    if (this._sensitivity === 'accent') return 0;
    if (ka === kb) return 0;
    // Lower case sorts before upper case, unless the locale asks otherwise.
    const upperFirst = this._caseFirst === 'upper';
    for (let i = 0; i < Math.min(ka.length, kb.length); i++) {
      if (ka[i] === kb[i]) continue;
      const aUpper = ka[i] === ka[i].toUpperCase();
      const bUpper = kb[i] === kb[i].toUpperCase();
      if (aUpper !== bUpper) return (aUpper === upperFirst) ? -1 : 1;
      return ka[i] < kb[i] ? -1 : 1;
    }
    return ka.length < kb.length ? -1 : 1;
  }

  resolvedOptions() {
    return {
      locale: this._locale,
      usage: this._usage,
      sensitivity: this._sensitivity,
      ignorePunctuation: this._ignorePunctuation,
      collation: 'default',
      numeric: this._numeric,
      caseFirst: this._caseFirst,
    };
  }

  static supportedLocalesOf(locales) {
    return supportedLocalesOf(locales);
  }
}

class Locale {
  constructor(tag, options) {
    const o = requireOptions(options);
    const canon = canonicalTag(typeof tag === 'object' && tag !== null ? tag.toString() : tag);
    const parts = canon.split('-');
    this.language = o.language ? String(o.language).toLowerCase() : parts[0];
    let rest = parts.slice(1);
    this.script = undefined;
    this.region = undefined;
    for (const p of rest) {
      if (p.length === 4 && this.script === undefined) this.script = p;
      else if ((p.length === 2 || p.length === 3) && this.region === undefined) this.region = p;
    }
    if (o.script) this.script = String(o.script);
    if (o.region) this.region = String(o.region);
    this.calendar = o.calendar === undefined ? undefined : String(o.calendar);
    this.numberingSystem = o.numberingSystem === undefined ? undefined : String(o.numberingSystem);
    this.hourCycle = o.hourCycle === undefined ? undefined : String(o.hourCycle);
    this.caseFirst = o.caseFirst === undefined ? undefined : String(o.caseFirst);
    this.numeric = o.numeric === undefined ? undefined : !!o.numeric;
    this.collation = o.collation === undefined ? undefined : String(o.collation);
  }

  get [Symbol.toStringTag]() {
    return 'Intl.Locale';
  }

  get baseName() {
    return [this.language, this.script, this.region].filter(Boolean).join('-');
  }

  toString() {
    const extensions = [];
    if (this.calendar) extensions.push(`ca-${this.calendar}`);
    if (this.collation) extensions.push(`co-${this.collation}`);
    if (this.hourCycle) extensions.push(`hc-${this.hourCycle}`);
    if (this.caseFirst) extensions.push(`kf-${this.caseFirst}`);
    if (this.numeric) extensions.push('kn');
    if (this.numberingSystem) extensions.push(`nu-${this.numberingSystem}`);
    return extensions.length ? `${this.baseName}-u-${extensions.join('-')}` : this.baseName;
  }

  maximize() {
    const guess = bestFit(this.baseName) || DEFAULT_LOCALE;
    const [lang, region] = guess.split('-');
    const script = {
      ja: 'Jpan', ko: 'Kore', ru: 'Cyrl', ar: 'Arab', hi: 'Deva',
      'zh-CN': 'Hans', 'zh-TW': 'Hant',
    }[guess] || { zh: 'Hans' }[lang] || 'Latn';
    return new Locale(`${this.language}-${this.script || script}-${this.region || region}`);
  }

  minimize() {
    return new Locale(this.language);
  }
}

function supportedLocalesOf(locales) {
  const list = locales === undefined ? [] : Array.isArray(locales) ? locales : [locales];
  const out = [];
  for (const l of list) {
    const canon = canonicalTag(l);
    if (bestFit(canon)) out.push(canon);
  }
  return out;
}

function getCanonicalLocales(locales) {
  const list = locales === undefined ? [] : Array.isArray(locales) ? locales : [locales];
  return list.map((l) => {
    const c = canonicalTag(l);
    if (!c || !/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(c)) {
      throw new RangeError(`Incorrect locale information provided`);
    }
    return c;
  });
}

function supportedValuesOf(key) {
  switch (String(key)) {
    case 'calendar':
      return ['gregory'];
    case 'collation':
      return ['emoji', 'eor'];
    case 'currency':
      return Object.keys(localeData(DEFAULT_LOCALE).currencies).sort();
    case 'numberingSystem':
      return ['latn'];
    case 'timeZone':
      return binding.tzZones().slice().sort();
    case 'unit':
      return Object.keys(localeData(DEFAULT_LOCALE).units).sort();
    default:
      throw new RangeError(`Invalid key : ${key}`);
  }
}

const Intl = {
  NumberFormat,
  DateTimeFormat,
  PluralRules,
  RelativeTimeFormat,
  ListFormat,
  DisplayNames,
  Collator,
  Locale,
  getCanonicalLocales,
  supportedValuesOf,
};

// `new Intl.NumberFormat(...)` and `Intl.NumberFormat(...)` both make one.
for (const name of ['NumberFormat', 'DateTimeFormat', 'Collator']) {
  const Class = Intl[name];
  const callable = function (locales, options) {
    return new Class(locales, options);
  };
  callable.prototype = Class.prototype;
  callable.supportedLocalesOf = Class.supportedLocalesOf;
  Object.defineProperty(callable, 'name', { value: name, configurable: true });
  Intl[name] = callable;
}

module.exports = { Intl, resolve, localeData, NumberFormat, DateTimeFormat };
