'use strict';

// A plain constructor function, as Node's is: `iconv-lite` (and through it
// Express's body parsers) subclass it with `StringDecoder.call(this, enc)`.
function StringDecoder(encoding = 'utf8') {
  this.encoding = String(encoding).toLowerCase().replace('-', '');
  this._pending = [];
}
StringDecoder.prototype.write = function write(buf) {
  if (typeof buf === 'string') return buf;
  const bytes = this._pending.concat(Array.from(buf));
  this._pending = [];
  if (this.encoding === 'utf8') {
    // Keep an incomplete trailing sequence for the next write.
    let end = bytes.length;
    let i = end - 1;
    let back = 0;
    while (i >= 0 && back < 4 && (bytes[i] & 0xc0) === 0x80) { i--; back++; }
    if (i >= 0) {
      const b = bytes[i];
      const need = b >= 0xf0 ? 4 : b >= 0xe0 ? 3 : b >= 0xc0 ? 2 : 1;
      if (need > back + 1) end = i;
    }
    this._pending = bytes.slice(end);
    return Buffer.from(bytes.slice(0, end)).toString('utf8');
  }
  return Buffer.from(bytes).toString(this.encoding);
};
StringDecoder.prototype.end = function end(buf) {
  let s = buf ? this.write(buf) : '';
  if (this._pending.length) {
    s += Buffer.from(this._pending).toString(this.encoding);
    this._pending = [];
  }
  return s;
};

module.exports = { StringDecoder };
