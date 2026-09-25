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

// Key objects. Only secret (symmetric) keys hold material here: what HMAC
// signing needs, and what `jsonwebtoken` wraps every string secret in. There
// are no asymmetric algorithms, so parsing a public or private key refuses,
// which is how a library learns a string is a shared secret.
class KeyObject {
  constructor(type, material) {
    if (type !== 'secret' && type !== 'public' && type !== 'private') {
      throw new TypeError(`The argument 'type' is invalid. Received '${type}'`);
    }
    Object.defineProperty(this, '_type', { value: type });
    Object.defineProperty(this, '_material', { value: material });
  }
  get type() { return this._type; }
  get symmetricKeySize() { return this._type === 'secret' ? this._material.length : undefined; }
  get asymmetricKeyType() { return undefined; }
  get asymmetricKeyDetails() { return undefined; }
  export(options) {
    if (options && options.format === 'jwk') {
      return { kty: 'oct', k: this._material.toString('base64url') };
    }
    return Buffer.from(this._material);
  }
  equals(other) {
    return other instanceof KeyObject && other._type === this._type
      && Buffer.compare(this._material, other._material) === 0;
  }
  static from(key) { return key; }
}

function keyMaterial(key, encoding) {
  if (key instanceof KeyObject) return key._material;
  if (typeof key === 'string') return Buffer.from(key, encoding || 'utf8');
  if (ArrayBuffer.isView(key)) return Buffer.from(key.buffer, key.byteOffset, key.byteLength);
  if (key instanceof ArrayBuffer) return Buffer.from(key);
  const e = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, DataView, KeyObject, or CryptoKey.');
  e.code = 'ERR_INVALID_ARG_TYPE';
  throw e;
}

function createSecretKey(key, encoding) {
  return new KeyObject('secret', Buffer.from(keyMaterial(key, encoding)));
}

function unsupportedKey() {
  const e = new Error('error:1E08010C:DECODER routines::unsupported');
  e.code = 'ERR_OSSL_UNSUPPORTED';
  throw e;
}

function createHmac(alg, key) {
  return new Hash(alg, keyMaterial(key));
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
  KeyObject,
  createSecretKey,
  createPublicKey: unsupportedKey,
  createPrivateKey: unsupportedKey,
  getHashes: () => binding.getHashes(),
  webcrypto: { getRandomValues: (a) => binding.getRandomValues(a), randomUUID: () => binding.randomUUID(), subtle: {} },
  constants: {},
};
