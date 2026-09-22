'use strict';

class Hash {
  constructor(alg, key) {
    this._alg = String(alg).toLowerCase().replace(/-/g, '');
    this._key = key;
    this._parts = [];
    if (!binding.getHashes().includes(this._alg)) {
      const e = new Error('Digest method not supported');
      e.code = 'ERR_OSSL_EVP_UNSUPPORTED';
      throw e;
    }
  }
  update(data, enc) {
    this._parts.push(typeof data === 'string' ? Buffer.from(data, enc || 'utf8') : data);
    return this;
  }
  digest(enc) {
    return binding.digest(this._alg, this._parts, enc, this._key);
  }
  copy() {
    const h = new Hash(this._alg, this._key);
    h._parts = this._parts.slice();
    return h;
  }
}

function createHash(alg) {
  return new Hash(alg);
}

function createHmac(alg, key) {
  return new Hash(alg, typeof key === 'string' ? Buffer.from(key) : key);
}

function hash(alg, data, enc = 'hex') {
  return new Hash(alg).update(data).digest(enc);
}

module.exports = {
  createHash,
  createHmac,
  hash,
  randomBytes: (n, cb) => binding.randomBytes(n, cb),
  randomUUID: () => binding.randomUUID(),
  randomInt: (a, b, cb) => {
    const v = binding.randomInt(a, typeof b === 'function' ? undefined : b);
    const f = typeof b === 'function' ? b : cb;
    if (typeof f === 'function') { process.nextTick(f, null, v); return undefined; }
    return v;
  },
  getRandomValues: (a) => binding.getRandomValues(a),
  randomFillSync: (a) => binding.getRandomValues(a),
  timingSafeEqual: (a, b) => binding.timingSafeEqual(a, b),
  getHashes: () => binding.getHashes(),
  webcrypto: { getRandomValues: (a) => binding.getRandomValues(a), randomUUID: () => binding.randomUUID(), subtle: {} },
  constants: {},
};
