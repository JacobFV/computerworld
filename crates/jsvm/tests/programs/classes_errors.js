// Errors, classes, inheritance, custom errors
class ValidationError extends Error {
  constructor(field, message) {
    super(message);
    this.name = 'ValidationError';
    this.field = field;
  }
}
class NotFound extends Error {}
function validate(user) {
  if (!user.name) throw new ValidationError('name', 'Name is required');
  if (user.age < 0) throw new RangeError(`Invalid age: ${user.age}`);
  return true;
}
for (const u of [{ name: 'a', age: 1 }, { age: 3 }, { name: 'b', age: -1 }]) {
  try {
    console.log('valid', validate(u));
  } catch (e) {
    console.log(e instanceof ValidationError, e instanceof Error, e.name, e.message, e.field, String(e), e.stack.split('\n')[0]);
  }
}
const nf = new NotFound('missing thing');
console.log(nf.name, nf.message, nf instanceof NotFound, Object.prototype.toString.call(nf), nf.constructor.name);
console.log(`${nf}`, nf.toString(), Object.keys(nf), JSON.stringify(nf), JSON.stringify({ e: new Error('x') }));
const withCause = new Error('outer', { cause: 'inner reason' });
console.log(withCause.cause, 'cause' in withCause);
try { undefinedFunction(); } catch (e) { console.log(e.name, e.message, e.constructor === ReferenceError); }
try { (void 0).prop; } catch (e) { console.log(e.name + ': ' + e.message); }
try { [].reduce((a, b) => a); } catch (e) { console.log(e.message); }
try { new Array(-5); } catch (e) { console.log(e.name, e.message); }
try { JSON.parse('{"a":'); } catch (e) { console.log(e.name, e.message); }
try { null(); } catch (e) { console.log(e.message); }
try { const o = {}; o.method(); } catch (e) { console.log(e.message); }
try { let q = 1; q(); } catch (e) { console.log(e.message); }
try { class A { constructor() { this.x = 1; } } A(); } catch (e) { console.log(e.message); }
try { Symbol() + ''; } catch (e) { console.log(e.message); }
try { BigInt(1.5); } catch (e) { console.log(e.name, e.message); }
try { 'x'.repeat(-1); } catch (e) { console.log(e.name, e.message); }
try { (class { #p; static check(o) { return o.#p; } }).check({}); } catch (e) { console.log(e.message); }
try { Object.defineProperty(Object.freeze({}), 'x', { value: 1 }); } catch (e) { console.log(e.message); }
try { 'use strict'; Object.freeze([1]).push(2); } catch (e) { console.log(e.message); }
try { structuredClone(Symbol('s')); } catch (e) { console.log(e.name); }
try { decodeURIComponent('%E0%A4%A'); } catch (e) { console.log(e.name, e.message); }
try { throw { custom: true }; } catch ({ custom }) { console.log('destructured catch', custom); }
try { try { throw new Error('inner'); } finally { console.log('inner finally'); } } catch (e) { console.log('outer caught', e.message); }
function f() { try { return 'try'; } finally { console.log('finally runs before return'); } }
console.log(f());
function g() { try { throw 1; } catch { return 'caught no binding'; } }
console.log(g());
const err = new TypeError('typed');
console.log(err instanceof TypeError, err instanceof Error, Object.getPrototypeOf(TypeError) === Error, TypeError.prototype.name);
class Base { static create() { return new this(); } who() { return 'base'; } }
class Derived extends Base { who() { return 'derived>' + super.who(); } }
console.log(Derived.create().who(), Derived.create() instanceof Derived, Base.create().who());
class Temp {
  static #count = 0;
  #celsius;
  constructor(c) { this.#celsius = c; Temp.#count++; }
  get fahrenheit() { return this.#celsius * 9 / 5 + 32; }
  set fahrenheit(f) { this.#celsius = (f - 32) * 5 / 9; }
  static get count() { return Temp.#count; }
  toJSON() { return { c: this.#celsius }; }
}
const t = new Temp(100);
t.fahrenheit = 32;
console.log(t.fahrenheit, Temp.count, JSON.stringify(t), t, Object.getOwnPropertyNames(Temp.prototype));
const mixin = (B) => class extends B { mixed() { return 'mixed ' + this.constructor.name; } };
class M extends mixin(Base) {}
console.log(new M().mixed(), new M().who(), M.name, mixin(Base).name);
function Legacy(name) { this.name = name; }
Legacy.prototype.hello = function () { return 'hi ' + this.name; };
const l = new Legacy('old');
console.log(l.hello(), l instanceof Legacy, l.constructor === Legacy, l);
console.log(Object.getOwnPropertyDescriptor({ get x() { return 1; } }, 'x'), Object.getOwnPropertyDescriptor([1], 'length'));
const frozen = Object.freeze({ a: 1, nested: { b: 2 } });
frozen.a = 2; frozen.nested.b = 3;
console.log(frozen, Object.isFrozen(frozen), Object.isFrozen(frozen.nested));
const sealed = Object.seal({ a: 1 }); sealed.a = 5; sealed.b = 6; delete sealed.a;
console.log(sealed, Object.isSealed(sealed));
