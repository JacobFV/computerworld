'use strict';
// zlib: deflate/inflate/gzip/gunzip/unzip/raw and brotli, sync, callback and
// stream forms. Compression is cw-zlib's port of the zlib Node ships (Chromium's
// fork, with its string hashing), so outputs match Node byte for byte.

const { Transform } = require('stream');

const constants = {
  Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2, Z_FULL_FLUSH: 3, Z_FINISH: 4, Z_BLOCK: 5,
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2, Z_DATA_ERROR: -3,
  Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
  Z_NO_COMPRESSION: 0, Z_BEST_SPEED: 1, Z_BEST_COMPRESSION: 9, Z_DEFAULT_COMPRESSION: -1,
  Z_FILTERED: 1, Z_HUFFMAN_ONLY: 2, Z_RLE: 3, Z_FIXED: 4, Z_DEFAULT_STRATEGY: 0, ZLIB_VERNUM: 4865,
  DEFLATE: 1, INFLATE: 2, GZIP: 3, GUNZIP: 4, DEFLATERAW: 5, INFLATERAW: 6, UNZIP: 7,
  BROTLI_DECODE: 8, BROTLI_ENCODE: 9, ZSTD_COMPRESS: 10, ZSTD_DECOMPRESS: 11,
  Z_MIN_WINDOWBITS: 8, Z_MAX_WINDOWBITS: 15, Z_DEFAULT_WINDOWBITS: 15,
  Z_MIN_CHUNK: 64, Z_MAX_CHUNK: Infinity, Z_DEFAULT_CHUNK: 16384,
  Z_MIN_MEMLEVEL: 1, Z_MAX_MEMLEVEL: 9, Z_DEFAULT_MEMLEVEL: 8,
  Z_MIN_LEVEL: -1, Z_MAX_LEVEL: 9, Z_DEFAULT_LEVEL: -1,
  BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1, BROTLI_OPERATION_FINISH: 2,
  BROTLI_OPERATION_EMIT_METADATA: 3, BROTLI_PARAM_MODE: 0, BROTLI_MODE_GENERIC: 0,
  BROTLI_MODE_TEXT: 1, BROTLI_MODE_FONT: 2, BROTLI_DEFAULT_MODE: 0, BROTLI_PARAM_QUALITY: 1,
  BROTLI_MIN_QUALITY: 0, BROTLI_MAX_QUALITY: 11, BROTLI_DEFAULT_QUALITY: 11, BROTLI_PARAM_LGWIN: 2,
  BROTLI_MIN_WINDOW_BITS: 10, BROTLI_MAX_WINDOW_BITS: 24, BROTLI_LARGE_MAX_WINDOW_BITS: 30,
  BROTLI_DEFAULT_WINDOW: 22, BROTLI_PARAM_LGBLOCK: 3, BROTLI_MIN_INPUT_BLOCK_BITS: 16,
  BROTLI_MAX_INPUT_BLOCK_BITS: 24, BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING: 4,
  BROTLI_PARAM_SIZE_HINT: 5, BROTLI_PARAM_LARGE_WINDOW: 6, BROTLI_PARAM_NPOSTFIX: 7,
  BROTLI_PARAM_NDIRECT: 8, BROTLI_DECODER_RESULT_ERROR: 0, BROTLI_DECODER_RESULT_SUCCESS: 1,
  BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT: 2, BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT: 3,
  BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION: 0, BROTLI_DECODER_PARAM_LARGE_WINDOW: 1,
  BROTLI_DECODER_NO_ERROR: 0, BROTLI_DECODER_SUCCESS: 1, BROTLI_DECODER_NEEDS_MORE_INPUT: 2,
  BROTLI_DECODER_NEEDS_MORE_OUTPUT: 3,
};
Object.freeze(constants);

const codes = {
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2, Z_ERRNO: -1, Z_STREAM_ERROR: -2, Z_DATA_ERROR: -3,
  Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
};
for (const k of Object.keys(codes)) codes[codes[k]] = k;

function received(v) {
  if (v === null) return 'null';
  if (v === undefined) return 'undefined';
  if (typeof v === 'function') return `function ${v.name}`;
  if (typeof v === 'object') return v.constructor && v.constructor.name ? `an instance of ${v.constructor.name}` : require('util').inspect(v);
  return `type ${typeof v} (${require('util').inspect(v)})`;
}

function toBuffer(buffer, name = 'buffer') {
  if (typeof buffer === 'string') return Buffer.from(buffer);
  if (Buffer.isBuffer(buffer)) return buffer;
  if (ArrayBuffer.isView(buffer)) return Buffer.from(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  if (buffer instanceof ArrayBuffer) return Buffer.from(new Uint8Array(buffer));
  const e = new TypeError(`The "${name}" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received ${received(buffer)}`);
  e.code = 'ERR_INVALID_ARG_TYPE';
  throw e;
}

function zlibError(raw) {
  const e = new Error(raw.message);
  e.errno = raw.errno;
  e.code = raw.code;
  return e;
}

function checkRange(name, v, min, max, def) {
  if (v === undefined || v === null) return def;
  if (typeof v !== 'number' || Number.isNaN(v)) {
    const e = new TypeError(`The "options.${name}" property must be of type number. Received ${received(v)}`);
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  if (v < min || v > max) {
    const e = new RangeError(`The value of "options.${name}" is out of range. It must be >= ${min} and <= ${max}. Received ${v}`);
    e.code = 'ERR_OUT_OF_RANGE';
    throw e;
  }
  return v;
}

const MODES = { 1: 'Deflate', 2: 'Inflate', 3: 'Gzip', 4: 'Gunzip', 5: 'DeflateRaw', 6: 'InflateRaw', 7: 'Unzip', 8: 'BrotliDecompress', 9: 'BrotliCompress' };

// One engine per stream or one-shot call.
class Engine {
  constructor(mode, opts = {}) {
    this.mode = mode;
    this.chunkSize = checkRange('chunkSize', opts.chunkSize, 64, Infinity, 16384);
    this.finishFlush = opts.finishFlush === undefined ? (mode >= 8 ? 2 : 4) : opts.finishFlush;
    this.flushFlag = opts.flush === undefined ? 0 : opts.flush;
    this.maxOutputLength = opts.maxOutputLength === undefined ? 0x1fffffffffffff : opts.maxOutputLength;
    if (mode >= 8) {
      const params = opts.params || {};
      this.brotli = {
        quality: params[constants.BROTLI_PARAM_QUALITY] !== undefined ? params[constants.BROTLI_PARAM_QUALITY] : 11,
        lgwin: params[constants.BROTLI_PARAM_LGWIN] !== undefined ? params[constants.BROTLI_PARAM_LGWIN] : 22,
        mode: params[constants.BROTLI_PARAM_MODE] !== undefined ? params[constants.BROTLI_PARAM_MODE] : 0,
        hint: params[constants.BROTLI_PARAM_SIZE_HINT] || 0,
      };
      this.pending = [];
      return;
    }
    const windowBits = checkRange('windowBits', opts.windowBits, mode === 7 || mode === 2 || mode === 4 || mode === 6 ? 0 : 8, 15, 15);
    const level = checkRange('level', opts.level, -1, 9, -1);
    const memLevel = checkRange('memLevel', opts.memLevel, 1, 9, 8);
    const strategy = checkRange('strategy', opts.strategy, 0, 4, 0);
    const dict = opts.dictionary === undefined ? undefined : toBuffer(opts.dictionary, 'options.dictionary');
    if (mode === 1 || mode === 3 || mode === 5) {
      const wrap = mode === 1 ? 1 : mode === 3 ? 2 : 0;
      this.handle = binding.zlibDeflateNew(level, windowBits || 15, memLevel, strategy, wrap, dict);
      this.deflate = true;
    } else {
      const wb = windowBits || 15;
      const bits = mode === 6 ? -wb : mode === 4 ? wb + 16 : mode === 7 ? wb + 32 : (windowBits === 0 ? 0 : wb);
      this.handle = binding.zlibInflateNew(bits, mode === 4 || mode === 7, dict);
      this.deflate = false;
    }
    if (this.handle === null) {
      const e = new Error('Init error');
      e.code = 'ERR_ZLIB_INITIALIZATION_FAILED';
      throw e;
    }
  }
  process(chunk, flush) {
    if (this.mode >= 8) {
      if (chunk.length) this.pending.push(chunk);
      if (flush !== 2) return Buffer.alloc(0);
      const input = Buffer.concat(this.pending);
      this.pending = [];
      if (this.mode === 9) return binding.brotliCompress(input, this.brotli.quality, this.brotli.lgwin, this.brotli.mode, this.brotli.hint || input.length);
      try {
        return binding.brotliDecompress(input);
      } catch (raw) {
        throw zlibError(raw);
      }
    }
    if (this.deflate) {
      try {
        return binding.zlibDeflate(this.handle, chunk, flush, this.chunkSize);
      } catch (raw) {
        throw zlibError(raw);
      }
    }
    let r;
    try {
      r = binding.zlibInflate(this.handle, chunk, flush === 4);
    } catch (raw) {
      throw zlibError(raw);
    }
    return r.out;
  }
  close() {
    if (this.handle !== undefined) binding.handleClose(this.handle);
  }
}

// Simulated time a threadpool job takes: dispatch plus work proportional to the
// bytes it handles (compression is slower per byte than decompression). Async
// completions land in that order, as they do in Node.
function workMs(mode, inBytes, outBytes) {
  const compress = mode === 1 || mode === 3 || mode === 5 || mode === 9;
  const perByte = mode === 9 ? 0.0002 : compress ? 0.00002 : 0.000004;
  return 0.05 + (compress ? inBytes : outBytes) * perByte;
}

function checkMax(out, engine) {
  if (out.length > engine.maxOutputLength) {
    const e = new RangeError(`Cannot create a Buffer larger than ${engine.maxOutputLength} bytes`);
    e.code = 'ERR_BUFFER_TOO_LARGE';
    throw e;
  }
  return out;
}

function syncFn(mode) {
  return function zlibSync(buffer, opts) {
    const buf = toBuffer(buffer);
    const engine = new Engine(mode, opts || {});
    try {
      const out = engine.process(buf, engine.finishFlush);
      checkMax(out, engine);
      if (opts && opts.info) return { buffer: out, engine: new Zlib(mode, opts) };
      return out;
    } finally {
      engine.close();
    }
  };
}

function asyncFn(sync, mode) {
  return function zlibAsync(buffer, opts, callback) {
    if (typeof opts === 'function') { callback = opts; opts = {}; }
    if (typeof callback !== 'function') {
      const e = new TypeError(`The "callback" argument must be of type function. Received ${received(callback)}`);
      e.code = 'ERR_INVALID_ARG_TYPE';
      throw e;
    }
    let result;
    let error = null;
    const input = toBuffer(buffer);
    try {
      result = sync(input, opts);
    } catch (e) {
      if (e.code === 'ERR_INVALID_ARG_TYPE') throw e;
      error = e;
    }
    const ms = workMs(mode, input.length, result ? result.length : 0);
    binding.scheduleIo(() => (error ? callback(error) : callback(null, result)), ms);
  };
}

class Zlib extends Transform {
  constructor(mode, opts = {}) {
    const engine = new Engine(mode, opts);
    super({
      transform(chunk, enc, cb) {
        let out;
        const input = typeof chunk === 'string' ? Buffer.from(chunk, enc) : toBuffer(chunk, 'chunk');
        try {
          out = engine.process(input, engine.flushFlag);
        } catch (e) {
          return cb(e);
        }
        emitChunks(this, out, engine.chunkSize);
        cb();
      },
      flush(cb) {
        let out;
        try {
          out = engine.process(Buffer.alloc(0), engine.finishFlush);
        } catch (e) {
          this.emit('error', e);
          return cb();
        }
        // The stream ends when the threadpool has done its work.
        binding.scheduleIo(() => {
          emitChunks(this, out, engine.chunkSize, true);
          cb();
        }, workMs(mode, this.bytesWritten, out.length + this._outSoFar));
      },
    });
    this._engine = engine;
    this._mode = mode;
    this._outSoFar = 0;
    this.bytesWritten = 0;
    this._handle = engine.handle;
  }
  get _closed() { return this._engine.handle === undefined; }
  params(level, strategy, callback) {
    const out = binding.zlibParams(this._engine.handle, level, strategy);
    if (out.length) this.push(out);
    if (callback) process.nextTick(callback);
  }
  flush(kind, callback) {
    if (typeof kind === 'function' || kind === undefined) { callback = kind; kind = constants.Z_FULL_FLUSH; }
    const out = this._engine.process(Buffer.alloc(0), kind);
    if (out.length) this.push(out);
    if (callback) process.nextTick(callback);
  }
  reset() {}
  close(callback) {
    this._engine.close();
    if (callback) process.nextTick(callback);
    this.destroy();
  }
  write(chunk, enc, cb) {
    if (chunk && chunk.length) this.bytesWritten += typeof chunk === 'string' ? Buffer.byteLength(chunk, enc) : chunk.length;
    return super.write(chunk, enc, cb);
  }
}

function emitChunks(stream, out, size) {
  stream._outSoFar = (stream._outSoFar || 0) + out.length;
  for (let i = 0; i < out.length; i += size) stream.push(out.subarray(i, Math.min(out.length, i + size)));
}

const classes = {};
const exportsObj = { constants, codes, Zlib };
for (const [m, name] of Object.entries(MODES)) {
  const mode = Number(m);
  const Klass = { [name]: class extends Zlib { constructor(opts) { super(mode, opts); } } }[name];
  classes[name] = Klass;
  exportsObj[name] = Klass;
  const lower = name[0].toLowerCase() + name.slice(1);
  exportsObj[`create${name}`] = (opts) => new Klass(opts);
  const sync = syncFn(mode);
  exportsObj[`${lower}Sync`] = sync;
  exportsObj[lower] = asyncFn(sync, mode);
}

exportsObj.crc32 = function crc32(data, value = 0) {
  return binding.crc32(typeof data === 'string' ? Buffer.from(data) : toBuffer(data, 'data'), value);
};

for (const k of Object.keys(constants)) {
  if (k.startsWith('Z_') && !(k in exportsObj)) {
    Object.defineProperty(exportsObj, k, { value: constants[k], enumerable: false, writable: false });
  }
}

module.exports = exportsObj;
