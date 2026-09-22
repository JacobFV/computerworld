'use strict';

const util = require('util');
const { inspect } = util;

const ARROW_ASSERT = 'node:assert:152\n  throw new AssertionError(obj);\n  ^\n';
const ARROW_OK = 'node:internal/assert/utils:77\n    throw err;\n    ^\n';

class AssertionError extends Error {
  constructor(options) {
    if (options === null || typeof options !== 'object') {
      const e = new TypeError('The "options" argument must be of type object. Received ' + inspect(options));
      e.code = 'ERR_INVALID_ARG_TYPE';
      throw e;
    }
    const { message, operator, stackStartFn, details } = options;
    let { actual, expected } = options;
    const generated = message == null;
    super(generated ? generateMessage(actual, expected, operator) : String(message));
    this.generatedMessage = generated;
    this.code = 'ERR_ASSERTION';
    if (details) {
      this.actual = undefined;
      this.expected = undefined;
      this.operator = undefined;
    } else {
      this.actual = actual;
      this.expected = expected;
      this.operator = operator;
    }
    this.diff = 'simple';
    Error.captureStackTrace(this, stackStartFn || AssertionError);
    Object.defineProperty(this, 'name', { value: 'AssertionError [ERR_ASSERTION]', enumerable: false, writable: true, configurable: true });
    this.stack; // eslint-disable-line no-unused-expressions
    delete this.name;
  }
  toString() {
    return `${this.name} [${this.code}]: ${this.message}`;
  }
}
AssertionError.prototype.name = 'AssertionError';

function inspectDiff(v) {
  return inspect(v, { compact: false, customInspect: false, depth: 1000, maxArrayLength: Infinity, sorted: true, getters: true, breakLength: Infinity });
}

function inspectShort(v) {
  return inspect(v, { compact: false, customInspect: false, depth: 1000, maxArrayLength: Infinity, sorted: true, getters: true });
}

function lcsDiff(a, b) {
  const n = a.length;
  const m = b.length;
  const dp = [];
  for (let i = 0; i <= n; i++) dp.push(new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out = [];
  let i = 0;
  let j = 0;
  let plus = [];
  let minus = [];
  const flush = () => {
    for (const l of plus) out.push('+ ' + l);
    for (const l of minus) out.push('- ' + l);
    plus = [];
    minus = [];
  };
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      flush();
      out.push('  ' + a[i]);
      i++;
      j++;
    } else if (j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j])) {
      minus.push(b[j]);
      j++;
    } else {
      plus.push(a[i]);
      i++;
    }
  }
  flush();
  return out;
}

const kReadableOperator = {
  deepStrictEqual: 'Expected values to be strictly deep-equal:',
  strictEqual: 'Expected values to be strictly equal:',
  strictEqualObject: 'Expected "actual" to be reference-equal to "expected":',
  deepEqual: 'Expected values to be loosely deep-equal:',
  notDeepStrictEqual: 'Expected "actual" not to be strictly deep-equal to:',
  notStrictEqual: 'Expected "actual" to be strictly unequal to:',
  notStrictEqualObject: 'Expected "actual" not to be reference-equal to "expected":',
  notDeepEqual: 'Expected "actual" not to be loosely deep-equal to:',
  notIdentical: 'Values identical but not reference-equal:',
  notDeepEqualUnequal: 'Expected values not to be loosely deep-equal:',
};

function generateMessage(actual, expected, operator) {
  if (operator === 'deepStrictEqual' || operator === 'strictEqual') {
    const a = inspectDiff(actual);
    const b = inspectDiff(expected);
    if (operator === 'strictEqual' && a === b && typeof actual === 'object' && actual !== null) {
      return 'Values have same structure but are not reference-equal:\n\n' + inspectShort(actual) + '\n';
    }
    const al = a.split('\n');
    const bl = b.split('\n');
    if (al.length === 1 && bl.length === 1 && a.length + b.length < 80 && (typeof actual !== 'object' || actual === null) && (typeof expected !== 'object' || expected === null)) {
      return `${kReadableOperator[operator]}\n\n${a} !== ${b}\n`;
    }
    const lines = lcsDiff(al, bl);
    return `${kReadableOperator[operator]}\n+ actual - expected\n\n${lines.join('\n')}\n`;
  }
  if (operator === 'deepEqual') {
    return `${kReadableOperator.deepEqual}\n\n${inspectShort(actual)}\n\nshould loosely deep-equal\n\n${inspectShort(expected)}`;
  }
  if (operator === 'notStrictEqual' || operator === 'notDeepStrictEqual' || operator === 'notDeepEqual') {
    const s = inspectShort(actual);
    const lines = s.split('\n');
    const base = kReadableOperator[operator];
    if (lines.length > 1) return `${base}\n\n${s}\n`;
    return `${base} ${s}`;
  }
  if (operator === '==') return `${inspect(actual)} == ${inspect(expected)}`;
  if (operator === '!=') return `${inspect(actual)} != ${inspect(expected)}`;
  if (operator === 'fail') return 'Failed';
  return `${inspect(actual)} ${operator} ${inspect(expected)}`;
}

function innerFail(obj, arrow) {
  if (obj.message instanceof Error) throw obj.message;
  const err = new AssertionError(obj);
  binding.setErrorFrames(err, arrow || ARROW_ASSERT, 0);
  throw err;
}

function sourceOfCall(depth) {
  const cs = binding.callsite(depth);
  if (!cs) return null;
  const line = cs.line;
  let start = cs.col - 1;
  while (start > 0 && /[\w$.]/.test(line[start - 1])) start--;
  let i = line.indexOf('(', cs.col - 1);
  if (i < 0) return null;
  let d = 0;
  for (; i < line.length; i++) {
    if (line[i] === '(') d++;
    else if (line[i] === ')') {
      d--;
      if (d === 0) break;
    }
  }
  return line.slice(start, i + 1);
}

function innerOk(fn, argLen, value, message, depth) {
  if (!value) {
    let generatedMessage = false;
    if (argLen === 0) {
      generatedMessage = true;
      message = 'No value argument passed to `assert.ok()`';
    } else if (message == null) {
      generatedMessage = true;
      const src = sourceOfCall(depth);
      message = src ? `The expression evaluated to a falsy value:\n\n  ${src}\n` : 'The expression evaluated to a falsy value';
    } else if (message instanceof Error) {
      throw message;
    }
    const err = new AssertionError({ actual: value, expected: true, message, operator: '==', stackStartFn: fn });
    err.generatedMessage = generatedMessage;
    binding.setErrorFrames(err, ARROW_OK, 0);
    throw err;
  }
}

function ok(...args) {
  innerOk(ok, args.length, args[0], args[1], 3);
}

const assert = ok;

assert.ok = function ok(...args) {
  innerOk(assert.ok, args.length, args[0], args[1], 3);
};

assert.fail = function fail(message) {
  if (message instanceof Error) throw message;
  const err = new AssertionError({ message: message === undefined ? 'Failed' : message, operator: 'fail', stackStartFn: fail });
  if (message === undefined) err.generatedMessage = true;
  binding.setErrorFrames(err, ARROW_ASSERT, 0);
  throw err;
};

assert.equal = function equal(actual, expected, message) {
  if (!(actual == expected || (actual !== actual && expected !== expected))) {
    innerFail({ actual, expected, message, operator: '==', stackStartFn: equal });
  }
};

assert.notEqual = function notEqual(actual, expected, message) {
  if (actual == expected || (actual !== actual && expected !== expected)) {
    innerFail({ actual, expected, message, operator: '!=', stackStartFn: notEqual });
  }
};

assert.strictEqual = function strictEqual(actual, expected, message) {
  if (!Object.is(actual, expected)) {
    innerFail({ actual, expected, message, operator: 'strictEqual', stackStartFn: strictEqual });
  }
};

assert.notStrictEqual = function notStrictEqual(actual, expected, message) {
  if (Object.is(actual, expected)) {
    innerFail({ actual, expected, message, operator: 'notStrictEqual', stackStartFn: notStrictEqual });
  }
};

assert.deepEqual = function deepEqual(actual, expected, message) {
  if (!util._deepEqual(actual, expected, false, new Map())) {
    innerFail({ actual, expected, message, operator: 'deepEqual', stackStartFn: deepEqual });
  }
};

assert.notDeepEqual = function notDeepEqual(actual, expected, message) {
  if (util._deepEqual(actual, expected, false, new Map())) {
    innerFail({ actual, expected, message, operator: 'notDeepEqual', stackStartFn: notDeepEqual });
  }
};

assert.deepStrictEqual = function deepStrictEqual(actual, expected, message) {
  if (!util.isDeepStrictEqual(actual, expected)) {
    innerFail({ actual, expected, message, operator: 'deepStrictEqual', stackStartFn: deepStrictEqual });
  }
};

assert.notDeepStrictEqual = function notDeepStrictEqual(actual, expected, message) {
  if (util.isDeepStrictEqual(actual, expected)) {
    innerFail({ actual, expected, message, operator: 'notDeepStrictEqual', stackStartFn: notDeepStrictEqual });
  }
};

function checkError(actual, expected, message, fn) {
  if (expected === undefined) return;
  if (typeof expected === 'function') {
    if (expected.prototype !== undefined && actual instanceof expected) return;
    if (Error.isPrototypeOf(expected) || expected === Error) {
      const name = actual && actual.constructor ? actual.constructor.name : typeof actual;
      const msg = message || `The error is expected to be an instance of "${expected.name}". Received "${name}"\n\nError message:\n\n${actual && actual.message}`;
      innerFail({ actual, expected, message: msg, operator: fn.name, stackStartFn: fn });
    }
    const r = expected.call({}, actual);
    if (r !== true) {
      const msg = message || `The ${expected.name ? `"${expected.name}" validation function` : 'validation function'} is expected to return "true". Received ${inspect(r)}\n\nCaught error:\n\n${actual}`;
      innerFail({ actual, expected, message: msg, operator: fn.name, stackStartFn: fn });
    }
    return;
  }
  if (expected instanceof RegExp) {
    const str = String(actual);
    if (expected.test(str)) return;
    innerFail({ actual, expected, message: message || `The input did not match the regular expression ${inspect(expected)}. Input:\n\n${inspect(str)}\n`, operator: fn.name, stackStartFn: fn });
  }
  if (typeof expected === 'object' && expected !== null) {
    for (const key of Object.keys(expected)) {
      const ev = expected[key];
      const av = actual == null ? undefined : actual[key];
      if (ev instanceof RegExp && typeof av === 'string' ? !ev.test(av) : !util.isDeepStrictEqual(av, ev)) {
        const lines = lcsDiff(inspectDiff(actual == null ? actual : pick(actual, Object.keys(expected))).split('\n'), inspectDiff(expected).split('\n'));
        innerFail({
          actual,
          expected,
          message: message || `Expected values to be strictly deep-equal:\n+ actual - expected\n\n${lines.join('\n')}\n`,
          operator: fn.name,
          stackStartFn: fn,
        });
      }
    }
  }
}

function pick(obj, keys) {
  const out = {};
  for (const k of keys) if (k in obj) out[k] = obj[k];
  return out;
}

assert.throws = function throws(fn, expected, message) {
  if (typeof fn !== 'function') {
    const e = new TypeError('The "fn" argument must be of type function. Received ' + inspect(fn));
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  if (typeof expected === 'string') {
    message = expected;
    expected = undefined;
  }
  try {
    fn();
  } catch (e) {
    checkError(e, expected, message, throws);
    return;
  }
  let details = '';
  if (expected && expected.name) details += ` (${expected.name})`;
  details += message ? `: ${message}` : '.';
  innerFail({ actual: undefined, expected, operator: 'throws', message: `Missing expected exception${details}`, stackStartFn: throws });
};

assert.doesNotThrow = function doesNotThrow(fn, expected, message) {
  try {
    fn();
  } catch (e) {
    if (typeof expected === 'string') message = expected;
    innerFail({ actual: e, expected, operator: 'doesNotThrow', message: `Got unwanted exception${message ? `: ${message}` : '.'}\nActual message: "${e && e.message}"`, stackStartFn: doesNotThrow });
  }
};

assert.rejects = async function rejects(promiseFn, expected, message) {
  let p = typeof promiseFn === 'function' ? promiseFn() : promiseFn;
  try {
    await p;
  } catch (e) {
    checkError(e, expected, message, rejects);
    return;
  }
  let details = '';
  if (expected && expected.name) details += ` (${expected.name})`;
  details += message ? `: ${message}` : '.';
  innerFail({ actual: undefined, expected, operator: 'rejects', message: `Missing expected rejection${details}`, stackStartFn: rejects });
};

assert.doesNotReject = async function doesNotReject(promiseFn, expected, message) {
  try {
    await (typeof promiseFn === 'function' ? promiseFn() : promiseFn);
  } catch (e) {
    innerFail({ actual: e, expected, operator: 'doesNotReject', message: `Got unwanted rejection.\nActual message: "${e && e.message}"`, stackStartFn: doesNotReject });
  }
};

assert.match = function match(string, regexp, message) {
  if (typeof string !== 'string' || !regexp.test(string)) {
    innerFail({
      actual: string,
      expected: regexp,
      message: message || (typeof string !== 'string'
        ? `The "string" argument must be of type string. Received type ${typeof string} (${inspect(string)})`
        : `The input did not match the regular expression ${inspect(regexp)}. Input:\n\n${inspect(string)}\n`),
      operator: 'match',
      stackStartFn: match,
    });
  }
};

assert.doesNotMatch = function doesNotMatch(string, regexp, message) {
  if (typeof string === 'string' && regexp.test(string)) {
    innerFail({
      actual: string,
      expected: regexp,
      message: message || `The input was expected to not match the regular expression ${inspect(regexp)}. Input:\n\n${inspect(string)}\n`,
      operator: 'doesNotMatch',
      stackStartFn: doesNotMatch,
    });
  }
};

assert.ifError = function ifError(err) {
  if (err !== null && err !== undefined) {
    let message = 'ifError got unwanted exception: ';
    message += typeof err === 'object' && typeof err.message === 'string' ? (err.message.length === 0 && err.constructor ? err.constructor.name : err.message) : inspect(err);
    innerFail({ actual: err, expected: null, operator: 'ifError', message, stackStartFn: ifError });
  }
};

assert.AssertionError = AssertionError;
assert.strict = assert;

module.exports = assert;
