#!/usr/bin/env node
// Regenerates crates/jsvm/data/cldr.json from the host Node's ICU.
//
// Run manually (never from a test) when the locale list or the surface changes:
//
//     node crates/jsvm/tools/generate_cldr.js
//
// Everything here is read out of the host's own Intl: a pattern is recorded as
// the token sequence `formatToParts` produces for a probe value, so what the
// simulated runtime formats is what Node formats, without a copy of CLDR.
//
// Encoding: a pattern is one string. Literal text stands as it is; a field is
// `$` and a letter (`$y` year, `$M` month, `$d` day, `$E` weekday, `$G` era,
// `$h` hour, `$m` minute, `$s` second, `$S` fractional second, `$a` day period,
// `$v` time zone name, `$n` a number, `$c` a currency, `$%` a percent sign,
// `$+`/`$-` a sign); a literal `$` is `$$`.
'use strict';
const fs = require('fs');
const path = require('path');

const OUT = path.join(__dirname, '..', 'data', 'cldr.json');

const LOCALES = [
  'en-US', 'en-GB', 'de-DE', 'fr-FR', 'es-ES', 'it-IT', 'pt-BR',
  'ja-JP', 'zh-CN', 'zh-TW', 'ko-KR', 'ru-RU', 'ar-EG', 'hi-IN',
];

// A probe instant whose every field is distinguishable: 2026-03-09,
// a Monday, 05:07:08.123 UTC.
const PROBE = new Date(Date.UTC(2026, 2, 9, 5, 7, 8, 123));
const PROBE_PM = new Date(Date.UTC(2026, 2, 9, 15, 7, 8, 123));

const FIELD = {
  era: 'G', year: 'y', month: 'M', day: 'd', weekday: 'E', dayPeriod: 'a',
  hour: 'h', minute: 'm', second: 's', fractionalSecond: 'S', timeZoneName: 'v',
  currency: 'c', percentSign: '%', plusSign: '+', minusSign: '-',
  exponentSeparator: 'e', exponentMinusSign: '-', exponentInteger: 'x',
  unit: 'u', compact: 'k', nan: 'N', infinity: 'I', unknown: '?',
};

const NUMBER_PARTS = ['integer', 'group', 'decimal', 'fraction'];

const escape = (s) => s.replace(/\$/g, '$$$$');

// Parts whose text belongs to the pattern rather than to the value: the unit
// after a measure, the `K` of a compact number.
const TEXT_PARTS = ['literal', 'unit', 'compact'];

/** A `formatToParts` result as one encoded pattern string. */
function encode(parts, numberTypes = NUMBER_PARTS) {
  let out = '';
  for (const p of parts) {
    if (TEXT_PARTS.includes(p.type)) out += escape(p.value);
    else if (numberTypes.includes(p.type)) out += out.endsWith('$n') ? '' : '$n';
    else out += `$${FIELD[p.type] || '?'}`;
  }
  return out;
}

const MONTH_WIDTHS = { numeric: 'M', '2-digit': 'MM', short: 'MMM', long: 'MMMM', narrow: 'MMMMM' };
const WEEKDAY_WIDTHS = { short: 'E', long: 'EEEE', narrow: 'EEEEE' };

/** The option sets whose patterns are recorded, by CLDR-style skeleton key. */
function skeletons() {
  const out = new Map();
  const add = (key, opts) => out.set(key, opts);
  for (const [mw, mk] of Object.entries(MONTH_WIDTHS)) {
    add(mk, { month: mw });
    add(`${mk}d`, { month: mw, day: 'numeric' });
    add(`y${mk}`, { year: 'numeric', month: mw });
    add(`y${mk}d`, { year: 'numeric', month: mw, day: 'numeric' });
    for (const [ww, wk] of Object.entries(WEEKDAY_WIDTHS)) {
      add(`${mk}d${wk}`, { month: mw, day: 'numeric', weekday: ww });
      add(`y${mk}d${wk}`, { year: 'numeric', month: mw, day: 'numeric', weekday: ww });
    }
  }
  add('y', { year: 'numeric' });
  add('d', { day: 'numeric' });
  for (const [ww, wk] of Object.entries(WEEKDAY_WIDTHS)) add(wk, { weekday: ww });
  add('Gy', { era: 'short', year: 'numeric' });
  add('GyMMMd', { era: 'short', year: 'numeric', month: 'short', day: 'numeric' });
  for (const h12 of [true, false]) {
    const p = h12 ? 'h' : 'H';
    add(`${p}`, { hour: 'numeric', hour12: h12 });
    add(`${p}m`, { hour: 'numeric', minute: 'numeric', hour12: h12 });
    add(`${p}ms`, { hour: 'numeric', minute: 'numeric', second: 'numeric', hour12: h12 });
    add(`${p}msS`, {
      hour: 'numeric', minute: 'numeric', second: 'numeric',
      fractionalSecondDigits: 3, hour12: h12,
    });
    add(`${p}${p}mm`, { hour: '2-digit', minute: '2-digit', hour12: h12 });
    add(`${p}${p}mmss`, {
      hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: h12,
    });
    add(`${p}mv`, { hour: 'numeric', minute: 'numeric', timeZoneName: 'short', hour12: h12 });
    add(`${p}mvvvv`, { hour: 'numeric', minute: 'numeric', timeZoneName: 'long', hour12: h12 });
    add(`${p}msv`, {
      hour: 'numeric', minute: 'numeric', second: 'numeric',
      timeZoneName: 'short', hour12: h12,
    });
  }
  add('m', { minute: 'numeric' });
  add('s', { second: 'numeric' });
  add('ms', { minute: 'numeric', second: 'numeric' });
  return out;
}

/**
 * A pattern, with the month and weekday marked as the form they use: `$M`/`$E`
 * for the one a date uses, `$L`/`$C` for the stand-alone one several languages
 * inflect differently.
 */
function pattern(locale, opts, names, date = PROBE) {
  const f = new Intl.DateTimeFormat(locale, { timeZone: 'UTC', ...opts });
  const parts = f.formatToParts(date);
  let out = '';
  for (const p of parts) {
    if (TEXT_PARTS.includes(p.type)) {
      out += escape(p.value);
    } else if (p.type === 'month' && names) {
      const standalone = ['long', 'short', 'narrow']
        .some((w) => names.monthsAlone[w][2] === p.value && names.months[w][2] !== p.value);
      out += standalone ? '$L' : '$M';
    } else if (p.type === 'weekday' && names) {
      const standalone = ['long', 'short', 'narrow']
        .some((w) => names.weekdaysAlone[w][1] === p.value && names.weekdays[w][1] !== p.value);
      out += standalone ? '$C' : '$E';
    } else {
      out += `$${FIELD[p.type] || '?'}`;
    }
  }
  return out;
}

/** The widths a style resolves to, read back from what it formatted. */
function styleWidths(locale, opts, names) {
  const parts = new Intl.DateTimeFormat(locale, { timeZone: 'UTC', ...opts })
    .formatToParts(PROBE);
  const out = {};
  for (const p of parts) {
    switch (p.type) {
      case 'year':
        out.year = p.value.length <= 2 ? '2-digit' : 'numeric';
        break;
      case 'month':
        // The probe is March: a name tells the width, a number its padding.
        if (p.value === names.months.long[2]) out.month = 'long';
        else if (p.value === names.months.short[2]) out.month = 'short';
        else if (p.value === names.months.narrow[2]) out.month = 'narrow';
        else out.month = p.value.length > 1 ? '2-digit' : 'numeric';
        break;
      case 'day':
        out.day = p.value.length > 1 ? '2-digit' : 'numeric';
        break;
      case 'weekday':
        // The probe is a Monday.
        if (p.value === names.weekdays.long[1]) out.weekday = 'long';
        else if (p.value === names.weekdays.short[1]) out.weekday = 'short';
        else out.weekday = 'narrow';
        break;
      case 'hour':
        out.hour = p.value.length > 1 ? '2-digit' : 'numeric';
        break;
      case 'minute':
        out.minute = p.value.length > 1 ? '2-digit' : 'numeric';
        break;
      case 'second':
        out.second = p.value.length > 1 ? '2-digit' : 'numeric';
        break;
      case 'timeZoneName':
        out.timeZoneName = p.value.length > 5 ? 'long' : 'short';
        break;
      case 'era':
        out.era = 'short';
        break;
      case 'dayPeriod':
        out.dayPeriod = 'short';
        break;
      default:
        break;
    }
  }
  return out;
}

function dateNames(locale) {
  // CLDR has two sets of names: the one a full date uses (`format`) and the one
  // a month or weekday on its own uses (`stand-alone`). Both are recorded by
  // asking for them in those two contexts.
  const month = (w, alone) =>
    Array.from({ length: 12 }, (_, i) =>
      new Intl.DateTimeFormat(locale, alone
        ? { month: w, timeZone: 'UTC' }
        : { year: 'numeric', month: w, day: 'numeric', timeZone: 'UTC' })
        .formatToParts(new Date(Date.UTC(2026, i, 15)))
        .find((p) => p.type === 'month').value);
  // 2026-03-08 is a Sunday.
  const weekday = (w, alone) =>
    Array.from({ length: 7 }, (_, i) =>
      new Intl.DateTimeFormat(locale, alone
        ? { weekday: w, timeZone: 'UTC' }
        : { weekday: w, year: 'numeric', month: 'numeric', day: 'numeric', timeZone: 'UTC' })
        .formatToParts(new Date(Date.UTC(2026, 2, 8 + i)))
        .find((p) => p.type === 'weekday').value);
  const dayPeriod = (d) => {
    const p = new Intl.DateTimeFormat(locale, { hour: 'numeric', hour12: true, timeZone: 'UTC' })
      .formatToParts(d).find((x) => x.type === 'dayPeriod');
    return p ? p.value : '';
  };
  const era = (w) =>
    [new Date(Date.UTC(-500, 0, 1)), PROBE].map((d) => {
      const p = new Intl.DateTimeFormat(locale, { era: w, year: 'numeric', timeZone: 'UTC' })
        .formatToParts(d).find((x) => x.type === 'era');
      return p ? p.value : '';
    });
  return {
    months: {
      long: month('long', false), short: month('short', false), narrow: month('narrow', false),
    },
    monthsAlone: {
      long: month('long', true), short: month('short', true), narrow: month('narrow', true),
    },
    weekdays: {
      long: weekday('long', false), short: weekday('short', false),
      narrow: weekday('narrow', false),
    },
    weekdaysAlone: {
      long: weekday('long', true), short: weekday('short', true), narrow: weekday('narrow', true),
    },
    dayPeriods: [dayPeriod(PROBE), dayPeriod(PROBE_PM)],
    eras: { long: era('long'), short: era('short'), narrow: era('narrow') },
  };
}

/** `[positive, negative]` patterns of a number style. */
function shape(locale, opts, value = 12345.6) {
  const f = new Intl.NumberFormat(locale, opts);
  return [encode(f.formatToParts(value)), encode(f.formatToParts(-value))];
}

function numberData(locale) {
  const parts = new Intl.NumberFormat(locale, { maximumFractionDigits: 3 })
    .formatToParts(-1234567.891);
  const symbol = (t, d) => (parts.find((p) => p.type === t) || { value: d }).value;
  const groups = new Intl.NumberFormat(locale, { useGrouping: true })
    .formatToParts(1234567890)
    .filter((p) => p.type === 'integer')
    .map((p) => p.value.length)
    .reverse();
  const compact = (style) => {
    const out = {};
    for (let e = 3; e <= 14; e++) {
      const f = new Intl.NumberFormat(locale, { notation: 'compact', compactDisplay: style });
      const value = 1.2 * 10 ** e;
      const parts = f.formatToParts(value);
      // What the number was divided by: `万` is ten thousand, `क॰` ten million.
      let integerDigits = 0;
      for (const p of parts) if (p.type === 'integer') integerDigits += p.value.length;
      const divisor = Math.max(0, e - Math.max(0, integerDigits - 1));
      // The plural form of the unit's name depends on the number in front of
      // it, so both are recorded: `[other, one, divisor exponent]`.
      out[e] = [encode(parts), encode(f.formatToParts(10 ** e)), divisor];
    }
    return out;
  };
  return {
    symbols: {
      decimal: symbol('decimal', '.'),
      group: symbol('group', ','),
      minus: symbol('minusSign', '-'),
      plus: new Intl.NumberFormat(locale, { signDisplay: 'always' })
        .formatToParts(1).find((p) => p.type === 'plusSign').value,
      percent: new Intl.NumberFormat(locale, { style: 'percent' })
        .formatToParts(0.5).find((p) => p.type === 'percentSign').value,
      nan: new Intl.NumberFormat(locale).format(NaN),
      infinity: new Intl.NumberFormat(locale).format(Infinity),
      exponential: new Intl.NumberFormat(locale, { notation: 'scientific' })
        .formatToParts(12345).find((p) => p.type === 'exponentSeparator').value,
      // The ten digits of the locale's numbering system, in order.
      digits: Array.from({ length: 10 }, (_, i) => new Intl.NumberFormat(locale).format(i)).join(''),
    },
    numberingSystem: new Intl.NumberFormat(locale).resolvedOptions().numberingSystem,
    grouping: groups.length > 1 ? [groups[0], groups[1]] : [3, 3],
    // Some locales only group from five digits up.
    minGroupingDigits:
      new Intl.NumberFormat(locale).formatToParts(1000).some((p) => p.type === 'group') ? 1 : 2,
    decimal: shape(locale, { maximumFractionDigits: 3 }),
    percent: shape(locale, { style: 'percent' }, 0.4567),
    scientific: shape(locale, { notation: 'scientific' }),
    compact: { short: compact('short'), long: compact('long') },
  };
}

const CURRENCIES = [
  'USD', 'EUR', 'GBP', 'JPY', 'CNY', 'INR', 'BRL', 'RUB', 'KRW', 'CHF',
  'CAD', 'AUD', 'MXN', 'SEK', 'TWD', 'EGP',
];

function currencyData(locale) {
  const currencies = {};
  for (const code of CURRENCIES) {
    const digits = new Intl.NumberFormat('en', { style: 'currency', currency: code })
      .resolvedOptions().maximumFractionDigits;
    const one = (display) =>
      new Intl.NumberFormat(locale, { style: 'currency', currency: code, currencyDisplay: display })
        .formatToParts(1).find((p) => p.type === 'currency').value;
    currencies[code] = [
      one('symbol'),
      one('narrowSymbol'),
      new Intl.NumberFormat(locale, {
        style: 'currency', currency: code, currencyDisplay: 'name',
      }).formatToParts(12).find((p) => p.type === 'currency').value,
      digits,
    ];
  }
  return {
    currencies,
    // The affixes are a property of the locale, not of the currency: `XXX`
    // stands in for whichever one is formatted.
    // Probed with a symbol that is not a letter: CLDR's currency spacing adds
    // the space back when the symbol next to the number is alphabetic, which
    // the runtime does itself.
    currencyShapes: {
      symbol: shape(locale, { style: 'currency', currency: 'EUR' }),
      code: shape(locale, { style: 'currency', currency: 'XXX', currencyDisplay: 'code' }),
      name: shape(locale, { style: 'currency', currency: 'XXX', currencyDisplay: 'name' }),
    },
  };
}

const UNITS = [
  'byte', 'celsius', 'centimeter', 'day', 'fahrenheit', 'gigabyte', 'gram',
  'hour', 'kilogram', 'kilometer', 'kilometer-per-hour', 'liter', 'megabyte',
  'meter', 'mile', 'mile-per-hour', 'millisecond', 'minute', 'percent',
  'second', 'week', 'year',
];

function unitData(locale) {
  const out = {};
  for (const unit of UNITS) {
    out[unit] = {};
    for (const display of ['long', 'short', 'narrow']) {
      // One and many: a unit's pattern can depend on the plural category.
      out[unit][display] = [1, 2].map((v) =>
        encode(new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: display })
          .formatToParts(v)));
    }
  }
  return out;
}

const RELATIVE_UNITS = ['year', 'quarter', 'month', 'week', 'day', 'hour', 'minute', 'second'];

function relativeData(locale) {
  const out = {};
  for (const unit of RELATIVE_UNITS) {
    const always = new Intl.RelativeTimeFormat(locale, { numeric: 'always' });
    const auto = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
    const record = (f, v) =>
      encode(f.formatToParts(v, unit), ['integer', 'group', 'decimal', 'fraction']);
    out[unit] = { always: {}, auto: {} };
    // Enough samples to cover every plural category these locales use.
    for (const v of [-11, -5, -2, -1, 0, 1, 2, 5, 11, 21]) {
      out[unit].always[v] = record(always, v);
      out[unit].auto[v] = record(auto, v);
    }
  }
  return out;
}

function listData(locale) {
  const out = {};
  for (const type of ['conjunction', 'disjunction', 'unit']) {
    out[type] = {};
    for (const style of ['long', 'short', 'narrow']) {
      const f = new Intl.ListFormat(locale, { type, style });
      const lits = (items) =>
        f.formatToParts(items).filter((p) => p.type === 'literal').map((p) => p.value);
      out[type][style] = { two: lits(['A', 'B']), three: lits(['A', 'B', 'C']) };
    }
  }
  return out;
}

const DISPLAY_LANGUAGES = ['en', 'de', 'fr', 'es', 'it', 'pt', 'ja', 'zh', 'ko', 'ru', 'ar', 'hi', 'nl', 'sv', 'pl', 'tr'];
const DISPLAY_REGIONS = ['US', 'GB', 'DE', 'FR', 'ES', 'IT', 'BR', 'JP', 'CN', 'TW', 'KR', 'RU', 'EG', 'IN', 'CA', 'AU', 'MX', 'SE', 'CH', 'NL', 'ZZ'];
const DISPLAY_SCRIPTS = ['Latn', 'Cyrl', 'Arab', 'Hans', 'Hant', 'Jpan', 'Kore', 'Deva'];

function displayNames(locale) {
  const of = (type, codes) => {
    const d = new Intl.DisplayNames([locale], { type });
    const out = {};
    for (const c of codes) {
      try {
        const v = d.of(c);
        if (v !== undefined) out[c] = v;
      } catch {
        /* not a name this locale has */
      }
    }
    return out;
  };
  return {
    language: of('language', DISPLAY_LANGUAGES),
    region: of('region', DISPLAY_REGIONS),
    currency: of('currency', CURRENCIES),
    script: of('script', DISPLAY_SCRIPTS),
  };
}

/** What each locale's plural rules answer, as a check on the ported rules. */
function pluralSamples(locale) {
  const out = { cardinal: {}, ordinal: {} };
  const c = new Intl.PluralRules(locale);
  const o = new Intl.PluralRules(locale, { type: 'ordinal' });
  for (const n of [0, 1, 2, 3, 4, 5, 6, 11, 12, 13, 21, 22, 23, 101, 111, 1000000]) {
    out.cardinal[n] = c.select(n);
    out.ordinal[n] = o.select(n);
  }
  for (const n of [0.5, 1.5, 2.5]) out.cardinal[n] = c.select(n);
  return out;
}

// The zones whose localized names are recorded: the ones the covered locales
// are spoken in, plus UTC. Every other zone falls back to a `GMT+02:00` form,
// which is what ICU itself does for a zone with no name of its own.
const NAMED_ZONES = [
  'UTC', 'America/New_York', 'America/Chicago', 'America/Denver',
  'America/Los_Angeles', 'America/Sao_Paulo', 'Europe/London', 'Europe/Berlin',
  'Europe/Paris', 'Europe/Madrid', 'Europe/Rome', 'Europe/Lisbon',
  'Europe/Moscow', 'Africa/Cairo', 'Asia/Tokyo', 'Asia/Shanghai',
  'Asia/Taipei', 'Asia/Seoul', 'Asia/Kolkata', 'Asia/Dubai', 'Australia/Sydney',
];

/** `zone -> [[short, long] in standard time, [short, long] in daylight]`. */
function zoneNames(locale) {
  const out = {};
  // January and July: one of them is daylight time wherever there is any.
  const winter = new Date(Date.UTC(2026, 0, 15, 12));
  const summer = new Date(Date.UTC(2026, 6, 15, 12));
  for (const zone of NAMED_ZONES) {
    const at = (d, width) => {
      const p = new Intl.DateTimeFormat(locale, { timeZone: zone, timeZoneName: width })
        .formatToParts(d).find((x) => x.type === 'timeZoneName');
      return p ? p.value : '';
    };
    // Keyed by the offset in force, so the southern hemisphere's summer is not
    // mistaken for the northern one's winter.
    const offsetAt = (d) => {
      const s = new Intl.DateTimeFormat('en-US', { timeZone: zone, timeZoneName: 'longOffset' })
        .formatToParts(d).find((x) => x.type === 'timeZoneName').value;
      const m = /GMT([+-])(\d{2}):(\d{2})/.exec(s);
      if (!m) return 0;
      return (m[1] === '-' ? -1 : 1) * (Number(m[2]) * 3600 + Number(m[3]) * 60);
    };
    out[zone] = {};
    for (const d of [winter, summer]) {
      out[zone][offsetAt(d)] = [at(d, 'short'), at(d, 'long')];
    }
  }
  return out;
}

function build() {
  const skeletonSet = skeletons();
  const locales = {};
  for (const locale of LOCALES) {
    const names = dateNames(locale);
    const patterns = {};
    for (const [key, opts] of skeletonSet) patterns[key] = pattern(locale, opts, names);
    const styles = (kind) => {
      const out = {};
      for (const style of ['full', 'long', 'medium', 'short']) {
        out[style] = pattern(
          locale, kind === 'date' ? { dateStyle: style } : { timeStyle: style }, names,
        );
      }
      return out;
    };
    const combined = {};
    for (const d of ['full', 'long', 'medium', 'short']) {
      combined[d] = {};
      for (const t of ['full', 'long', 'medium', 'short']) {
        combined[d][t] = pattern(locale, { dateStyle: d, timeStyle: t }, names);
      }
    }
    const resolved = new Intl.DateTimeFormat(locale).resolvedOptions();
    const styleOptions = { date: {}, time: {}, combined: {} };
    for (const style of ['full', 'long', 'medium', 'short']) {
      styleOptions.date[style] = styleWidths(locale, { dateStyle: style }, names);
      styleOptions.time[style] = styleWidths(locale, { timeStyle: style }, names);
      styleOptions.combined[style] = {};
      for (const t of ['full', 'long', 'medium', 'short']) {
        styleOptions.combined[style][t] = styleWidths(
          locale, { dateStyle: style, timeStyle: t }, names,
        );
      }
    }
    locales[locale] = {
      ...names,
      styleOptions,
      patterns,
      dateStyles: styles('date'),
      timeStyles: styles('time'),
      dateTimeStyles: combined,
      hourCycle: new Intl.DateTimeFormat(locale, { hour: 'numeric' }).resolvedOptions().hourCycle,
      calendar: resolved.calendar,
      numberingSystem: resolved.numberingSystem,
      number: numberData(locale),
      ...currencyData(locale),
      units: unitData(locale),
      relative: relativeData(locale),
      list: listData(locale),
      displayNames: displayNames(locale),
      zoneNames: zoneNames(locale),
      plurals: pluralSamples(locale),
    };
  }
  return { icu: process.versions.icu, node: process.version, locales };
}

const data = build();
fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, `${JSON.stringify(data)}\n`);
process.stderr.write(`${OUT}: ${fs.statSync(OUT).size} bytes for ${LOCALES.length} locales\n`);
