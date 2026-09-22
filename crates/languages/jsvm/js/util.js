'use strict';

const inspect = function inspect(value, opts, depth, colors) {
  if (opts !== null && typeof opts === 'object') return binding.inspect(value, opts);
  if (typeof opts === 'boolean' || depth !== undefined) {
    return binding.inspect(value, { showHidden: !!opts, depth: depth === undefined ? 2 : depth });
  }
  return binding.inspect(value);
};
inspect.custom = Symbol.for('nodejs.util.inspect.custom');
inspect.defaultOptions = {
  showHidden: false,
  depth: 2,
  colors: false,
  customInspect: true,
  showProxy: false,
  maxArrayLength: 100,
  maxStringLength: 10000,
  breakLength: 80,
  compact: 3,
  sorted: false,
  getters: false,
  numericSeparator: false,
};
inspect.colors = { bold: [1, 22], italic: [3, 23], underline: [4, 24], inverse: [7, 27], white: [37, 39], grey: [90, 39], black: [30, 39], blue: [34, 39], cyan: [36, 39], green: [32, 39], magenta: [35, 39], red: [31, 39], yellow: [33, 39] };
inspect.styles = { special: 'cyan', number: 'yellow', bigint: 'yellow', boolean: 'yellow', undefined: 'grey', null: 'bold', string: 'green', symbol: 'green', date: 'magenta', regexp: 'red', module: 'underline' };

function format(...args) {
  return binding.format(...args);
}

function formatWithOptions(opts, ...args) {
  return binding.format(...args);
}

const kCustomPromisify = Symbol.for('nodejs.util.promisify.custom');

function promisify(original) {
  if (typeof original !== 'function') {
    const err = new TypeError('The "original" argument must be of type function. Received ' + inspect(original));
    err.code = 'ERR_INVALID_ARG_TYPE';
    throw err;
  }
  if (original[kCustomPromisify]) return original[kCustomPromisify];
  function fn(...args) {
    return new Promise((resolve, reject) => {
      Reflect.apply(original, this, [...args, (err, ...values) => {
        if (err) return reject(err);
        resolve(values.length > 1 ? values : values[0]);
      }]);
    });
  }
  Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
  Object.defineProperty(fn, kCustomPromisify, { value: fn, enumerable: false, writable: false, configurable: true });
  return Object.defineProperties(fn, Object.getOwnPropertyDescriptors(original));
}
promisify.custom = kCustomPromisify;

function callbackify(original) {
  return function (...args) {
    const cb = args.pop();
    original.apply(this, args).then((v) => process.nextTick(cb, null, v), (e) => process.nextTick(cb, e));
  };
}

function inherits(ctor, superCtor) {
  Object.defineProperty(ctor, 'super_', { value: superCtor, writable: true, configurable: true });
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
}

const warned = new Set();
function deprecate(fn, msg, code) {
  let warnedHere = false;
  function deprecated(...args) {
    if (!warnedHere && !(code && warned.has(code))) {
      warnedHere = true;
      if (code) warned.add(code);
      process.emitWarning(msg, code ? { type: 'DeprecationWarning', code } : 'DeprecationWarning');
    }
    return new.target ? Reflect.construct(fn, args, new.target) : Reflect.apply(fn, this, args);
  }
  Object.setPrototypeOf(deprecated, fn);
  return deprecated;
}

function isDeepStrictEqual(a, b) {
  return deepEqual(a, b, true, new Map());
}

function deepEqual(a, b, strict, memo) {
  if (strict ? Object.is(a, b) : a == b || (a !== a && b !== b)) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) {
    if (!strict && (typeof a !== 'object' || a === null) && (typeof b !== 'object' || b === null)) return a == b;
    return false;
  }
  if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  const ta = binding.typeTag(a);
  const tb = binding.typeTag(b);
  if (ta !== tb) return false;
  if (memo.has(a) && memo.get(a) === b) return true;
  memo.set(a, b);
  if (ta === 'Date') return a.getTime() === b.getTime();
  if (ta === 'RegExp') return a.source === b.source && a.flags === b.flags && a.lastIndex === b.lastIndex;
  if (ta === 'Boxed') return Object.is(a.valueOf(), b.valueOf());
  if (ta === 'Error') {
    if (a.message !== b.message || a.name !== b.name) return false;
  }
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      const ha = Object.prototype.hasOwnProperty.call(a, i);
      const hb = Object.prototype.hasOwnProperty.call(b, i);
      if (strict && ha !== hb) return false;
      if (!deepEqual(a[i], b[i], strict, memo)) return false;
    }
  } else if (ta === 'TypedArray') {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false;
  } else if (ta === 'Map') {
    if (a.size !== b.size) return false;
    for (const [k, v] of a) {
      if (!b.has(k)) {
        let found = false;
        if (typeof k === 'object' && k !== null) {
          for (const [k2, v2] of b) {
            if (deepEqual(k, k2, strict, memo) && deepEqual(v, v2, strict, memo)) { found = true; break; }
          }
        }
        if (!found) return false;
      } else if (!deepEqual(v, b.get(k), strict, memo)) return false;
    }
    return true;
  } else if (ta === 'Set') {
    if (a.size !== b.size) return false;
    outer: for (const v of a) {
      if (b.has(v)) continue;
      if (typeof v === 'object' && v !== null) {
        for (const w of b) if (deepEqual(v, w, strict, memo)) continue outer;
      }
      return false;
    }
    return true;
  }
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
    if (!deepEqual(a[k], b[k], strict, memo)) return false;
  }
  if (strict) {
    const sa = Object.getOwnPropertySymbols(a).filter((s) => Object.prototype.propertyIsEnumerable.call(a, s));
    const sb = Object.getOwnPropertySymbols(b).filter((s) => Object.prototype.propertyIsEnumerable.call(b, s));
    if (sa.length !== sb.length) return false;
    for (const s of sa) if (!deepEqual(a[s], b[s], strict, memo)) return false;
  }
  return true;
}

const tag = (v) => binding.typeTag(v);
const types = {
  isPromise: (v) => tag(v) === 'Promise',
  isDate: (v) => tag(v) === 'Date',
  isRegExp: (v) => tag(v) === 'RegExp',
  isMap: (v) => tag(v) === 'Map',
  isSet: (v) => tag(v) === 'Set',
  isWeakMap: (v) => tag(v) === 'WeakMap',
  isWeakSet: (v) => tag(v) === 'WeakSet',
  isNativeError: (v) => tag(v) === 'Error',
  isAsyncFunction: (v) => tag(v) === 'AsyncFunction' || tag(v) === 'AsyncGeneratorFunction',
  isGeneratorFunction: (v) => tag(v) === 'GeneratorFunction' || tag(v) === 'AsyncGeneratorFunction',
  isGeneratorObject: (v) => tag(v) === 'Generator' || tag(v) === 'AsyncGenerator',
  isTypedArray: (v) => tag(v) === 'TypedArray',
  isUint8Array: (v) => tag(v) === 'TypedArray' && v[Symbol.toStringTag] === 'Uint8Array',
  isArrayBuffer: (v) => tag(v) === 'ArrayBuffer',
  isAnyArrayBuffer: (v) => tag(v) === 'ArrayBuffer',
  isBoxedPrimitive: (v) => tag(v) === 'Boxed',
  isProxy: (v) => tag(v) === 'Proxy',
  isMapIterator: (v) => tag(v) === 'MapIterator',
  isSetIterator: (v) => tag(v) === 'SetIterator',
  isArgumentsObject: (v) => tag(v) === 'Arguments',
  isNumberObject: (v) => tag(v) === 'Boxed' && typeof v.valueOf() === 'number',
  isStringObject: (v) => tag(v) === 'Boxed' && typeof v.valueOf() === 'string',
};

function stripVTControlCharacters(str) {
  return String(str).replace(/\[[0-9;]*[A-Za-z]/g, '');
}

function styleText(format, text) {
  return text;
}

function parseArgs(config = {}) {
  const args = config.args || process.argv.slice(2);
  const options = config.options || {};
  const values = { __proto__: null };
  const positionals = [];
  const shortMap = {};
  for (const [name, o] of Object.entries(options)) {
    if (o.short) shortMap[o.short] = name;
    if (o.default !== undefined) values[name] = o.default;
  }
  const unknown = (a) => {
    const e = new TypeError(`Unknown option '${a}'`);
    e.code = 'ERR_PARSE_ARGS_UNKNOWN_OPTION';
    return e;
  };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '--') { positionals.push(...args.slice(i + 1)); break; }
    let name; let value;
    if (a.startsWith('--')) {
      const eq = a.indexOf('=');
      name = eq >= 0 ? a.slice(2, eq) : a.slice(2);
      value = eq >= 0 ? a.slice(eq + 1) : undefined;
    } else if (a.startsWith('-') && a.length > 1) {
      name = shortMap[a[1]];
      if (!name) { if (config.strict === false) { positionals.push(a); continue; } throw unknown(a); }
      if (a.length > 2) value = a.slice(2);
    } else {
      if (config.allowPositionals === false && config.strict !== false) {
        const e = new TypeError(`Unexpected argument '${a}'. This command does not take positional arguments`);
        e.code = 'ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL';
        throw e;
      }
      positionals.push(a);
      continue;
    }
    const o = options[name];
    if (!o) { if (config.strict === false) { values[name] = value === undefined ? true : value; continue; } throw unknown(a.startsWith('--') ? '--' + name : a); }
    if (o.type === 'string') {
      if (value === undefined) value = args[++i];
    } else {
      value = true;
    }
    if (o.multiple) (values[name] = values[name] && Array.isArray(values[name]) ? values[name] : []).push(value);
    else values[name] = value;
  }
  return { values, positionals };
}

function getSystemErrorName(n) {
  const names = { '-2': 'ENOENT', '-13': 'EACCES', '-17': 'EEXIST', '-20': 'ENOTDIR', '-21': 'EISDIR', '-22': 'EINVAL', '-39': 'ENOTEMPTY' };
  return names[n];
}

function debuglog() {
  const fn = () => {};
  fn.enabled = false;
  return fn;
}

module.exports = {
  inspect,
  format,
  formatWithOptions,
  promisify,
  callbackify,
  inherits,
  deprecate,
  isDeepStrictEqual,
  types,
  stripVTControlCharacters,
  styleText,
  parseArgs,
  getSystemErrorName,
  debuglog,
  debug: debuglog,
  isArray: Array.isArray,
  isBoolean: (v) => typeof v === 'boolean',
  isNull: (v) => v === null,
  isNullOrUndefined: (v) => v == null,
  isNumber: (v) => typeof v === 'number',
  isString: (v) => typeof v === 'string',
  isSymbol: (v) => typeof v === 'symbol',
  isUndefined: (v) => v === undefined,
  isObject: (v) => v !== null && typeof v === 'object',
  isFunction: (v) => typeof v === 'function',
  isPrimitive: (v) => v === null || (typeof v !== 'object' && typeof v !== 'function'),
  isRegExp: types.isRegExp,
  isDate: types.isDate,
  isError: (v) => types.isNativeError(v) || v instanceof Error,
  toUSVString: (s) => String(s),
  TextEncoder,
  TextDecoder,
  _deepEqual: deepEqual,
};
