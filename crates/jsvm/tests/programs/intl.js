// ECMA-402: dates, numbers, plurals, relative times, lists, display names,
// collation and locales, in the languages the simulated runtime carries.
const LOCALES = ['en-US', 'en-GB', 'de-DE', 'fr-FR', 'es-ES', 'it-IT', 'pt-BR',
  'ja-JP', 'zh-CN', 'zh-TW', 'ko-KR', 'ru-RU', 'ar-EG', 'hi-IN'];
const when = new Date(Date.UTC(2026, 10, 27, 15, 34, 56, 789));

console.log('--- dates');
for (const l of LOCALES) {
  const f = (o) => new Intl.DateTimeFormat(l, { timeZone: 'UTC', ...o }).format(when);
  console.log(l, [
    f({}),
    f({ dateStyle: 'full' }),
    f({ dateStyle: 'long', timeStyle: 'short' }),
    f({ dateStyle: 'short', timeStyle: 'medium' }),
    f({ weekday: 'short', month: 'short', day: 'numeric' }),
    f({ year: '2-digit', month: '2-digit', day: '2-digit' }),
    f({ hour: 'numeric', minute: '2-digit', hour12: true }),
    f({ hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23' }),
    f({ month: 'long', year: 'numeric' }),
  ].join(' | '));
}

console.log('--- zones');
for (const zone of ['UTC', 'America/New_York', 'Europe/Berlin', 'Asia/Tokyo',
  'Australia/Sydney', 'Asia/Kolkata', 'America/Sao_Paulo']) {
  const summer = new Date(Date.UTC(2026, 6, 15, 12));
  const winter = new Date(Date.UTC(2026, 0, 15, 12));
  const f = (d) => new Intl.DateTimeFormat('en-US', {
    timeZone: zone, dateStyle: 'medium', timeStyle: 'long',
  }).format(d);
  console.log(zone, f(winter), '|', f(summer));
}
console.log(new Intl.DateTimeFormat('de-DE', {
  timeZone: 'Europe/Berlin', dateStyle: 'full', timeStyle: 'full',
}).format(when));
console.log(JSON.stringify(new Intl.DateTimeFormat('en-US', {
  timeZone: 'UTC', dateStyle: 'medium', timeStyle: 'short',
}).formatToParts(when)));
try {
  new Intl.DateTimeFormat('en-US', { timeZone: 'Mars/Olympus' });
} catch (e) {
  console.log(e.constructor.name, e.message);
}

console.log('--- numbers');
for (const l of LOCALES) {
  const f = (v, o) => new Intl.NumberFormat(l, o).format(v);
  console.log(l, [
    f(1234567.891),
    f(-0.5, { style: 'percent' }),
    f(1234.5, { style: 'currency', currency: 'EUR' }),
    f(1234.5, { style: 'currency', currency: 'JPY' }),
    f(12345678, { notation: 'compact' }),
    f(1234, { notation: 'compact', compactDisplay: 'long' }),
    f(42, { style: 'unit', unit: 'kilometer' }),
    f(0.00001234, { maximumSignificantDigits: 3 }),
  ].join(' | '));
}
const nf = new Intl.NumberFormat('en-US');
console.log([
  nf.format(NaN), nf.format(Infinity), nf.format(-0),
  new Intl.NumberFormat('en-US', { minimumFractionDigits: 3 }).format(1.5),
  new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(2.5),
  new Intl.NumberFormat('en-US', { useGrouping: false }).format(1234567),
  new Intl.NumberFormat('en-US', { signDisplay: 'exceptZero' }).format(0),
  new Intl.NumberFormat('en-US', { notation: 'scientific' }).format(12345),
  new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', currencyDisplay: 'name' }).format(1),
  new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', currencyDisplay: 'code' }).format(-2),
  new Intl.NumberFormat('en-US', { minimumIntegerDigits: 3 }).format(7),
].join(' | '));
console.log(JSON.stringify(new Intl.NumberFormat('de-DE', {
  style: 'currency', currency: 'EUR',
}).formatToParts(-1234.5)));
console.log(JSON.stringify(new Intl.NumberFormat('en-US').resolvedOptions()));

console.log('--- plurals');
for (const l of LOCALES) {
  const c = new Intl.PluralRules(l);
  const o = new Intl.PluralRules(l, { type: 'ordinal' });
  console.log(l,
    [0, 1, 2, 3, 5, 11, 21, 101].map((n) => c.select(n)).join(','),
    '|', [1, 2, 3, 4, 11, 21].map((n) => o.select(n)).join(','));
}

console.log('--- relative');
for (const l of LOCALES) {
  const a = new Intl.RelativeTimeFormat(l, { numeric: 'auto' });
  const n = new Intl.RelativeTimeFormat(l);
  console.log(l, [
    a.format(-1, 'day'), a.format(0, 'day'), a.format(1, 'day'),
    n.format(-3, 'month'), n.format(5, 'minute'), n.format(1, 'year'),
  ].join(' | '));
}

console.log('--- lists');
for (const l of LOCALES) {
  console.log(l,
    new Intl.ListFormat(l).format(['x', 'y', 'z']),
    '|', new Intl.ListFormat(l, { type: 'disjunction' }).format(['x', 'y']),
    '|', new Intl.ListFormat(l, { type: 'unit', style: 'short' }).format(['x', 'y', 'z']));
}

console.log('--- names');
for (const l of ['en-US', 'de-DE', 'fr-FR', 'ja-JP', 'ru-RU']) {
  const region = new Intl.DisplayNames([l], { type: 'region' });
  const language = new Intl.DisplayNames([l], { type: 'language' });
  const currency = new Intl.DisplayNames([l], { type: 'currency' });
  console.log(l, region.of('JP'), language.of('ar'), currency.of('EUR'), region.of('ZZ'));
}

console.log('--- collation');
const words = ['zebra', 'Äpfel', 'apfel', 'Banane', 'äther', 'Zoo'];
for (const l of ['en-US', 'de-DE', 'fr-FR', 'es-ES']) {
  console.log(l, [...words].sort(new Intl.Collator(l).compare).join(','));
}
console.log([
  new Intl.Collator('en-US', { sensitivity: 'base' }).compare('a', 'Á'),
  new Intl.Collator('en-US').compare('a', 'b'),
  new Intl.Collator('en-US', { numeric: true }).compare('file10', 'file9'),
  'ä'.localeCompare('z', 'de-DE'),
].join(' '));

console.log('--- locales');
console.log(Intl.getCanonicalLocales(['DE-de', 'fr', 'ZH-hans-cn']).join(','));
const loc = new Intl.Locale('en-Latn-US', { calendar: 'gregory' });
console.log(loc.baseName, loc.language, loc.region, loc.script, String(loc));
console.log(new Intl.Locale('de').maximize().baseName, new Intl.Locale('ja-JP').minimize().baseName);
console.log(Intl.NumberFormat.supportedLocalesOf(['de-DE', 'xx-YY', 'ja']).join(','));
// The simulated runtime carries the Gregorian calendar and Latin digits; the
// lists themselves are ICU's inventory, which is not.
console.log(Intl.supportedValuesOf('calendar').includes('gregory'),
  Intl.supportedValuesOf('numberingSystem').includes('latn'));
console.log(Intl.supportedValuesOf('timeZone').includes('Europe/Berlin'),
  Intl.supportedValuesOf('currency').includes('EUR'));
console.log(typeof Intl.DateTimeFormat('en').format(when), new Intl.DateTimeFormat().resolvedOptions().locale);

console.log('--- prototypes');
console.log((1234.5678).toLocaleString('de-DE'),
  when.toLocaleDateString('fr-FR', { timeZone: 'UTC' }),
  when.toLocaleTimeString('en-GB', { timeZone: 'UTC' }),
  when.toLocaleString('ja-JP', { timeZone: 'UTC' }));
