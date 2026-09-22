const zlib = require('zlib');
const { promisify } = require('util');

const text = Buffer.from('The quick brown fox jumps over the lazy dog. '.repeat(40));
const words = Buffer.from(Array.from({ length: 3000 }, (_, i) => String((i * i) % 977)).join(' '));

for (const level of [-1, 0, 1, 6, 9]) {
  const c = zlib.deflateSync(words, { level });
  console.log(level, c.length, c.subarray(0, 6).toString('hex'), zlib.crc32(c));
}
for (const windowBits of [9, 12, 15]) {
  console.log('wb', windowBits, zlib.deflateSync(text, { windowBits }).toString('base64').slice(0, 40));
}
for (const strategy of [1, 2, 3, 4]) {
  console.log('strategy', strategy, zlib.deflateSync(words, { strategy }).toString('hex').slice(0, 40));
}
console.log(zlib.deflateRawSync(text).length, zlib.gzipSync(text).toString('hex').slice(0, 40));
console.log(zlib.gzipSync(text, { level: 9 }).subarray(0, 10).toString('hex'));
console.log(zlib.inflateSync(zlib.deflateSync(words)).equals(words));
console.log(zlib.unzipSync(zlib.gzipSync(words)).equals(words), zlib.unzipSync(zlib.deflateSync(text)).length);
console.log(zlib.gunzipSync(Buffer.concat([zlib.gzipSync('one '), zlib.gzipSync('two')])).toString());
console.log(zlib.inflateRawSync(zlib.deflateRawSync(text)).length);
const dict = Buffer.from('quick brown fox lazy dog');
const withDict = zlib.deflateSync(text, { dictionary: dict });
console.log(withDict.length, zlib.inflateSync(withDict, { dictionary: dict }).equals(text));
for (const bad of [Buffer.from('not zlib'), zlib.deflateSync(text).subarray(0, 20)]) {
  try { zlib.inflateSync(bad); } catch (e) { console.log(e.name, e.message, e.code, e.errno); }
}
try { zlib.inflateSync(withDict); } catch (e) { console.log(e.message, e.code, e.errno); }
try { zlib.deflateSync(42); } catch (e) { console.log(e.name, e.code, e.message); }
try { zlib.deflateSync('x', { level: 12 }); } catch (e) { console.log(e.name, e.code, e.message); }
console.log(zlib.crc32('hello'), zlib.crc32(Buffer.from('world'), 7), zlib.constants.Z_BEST_SPEED);
const br = zlib.brotliCompressSync(text);
console.log(zlib.brotliDecompressSync(br).equals(text), zlib.brotliCompressSync('abc abc abc').toString('hex'));

zlib.gzip(words, (err, out) => {
  console.log('gzip cb', err, out.length);
  promisify(zlib.deflate)(text).then((out2) => console.log('promisified', out2.length));
});
zlib.inflate(Buffer.from('broken'), (err) => console.log('inflate cb', err.message));

function inflateStream() {
  const inf = zlib.createInflate();
  const chunks = [];
  inf.on('data', (c) => chunks.push(c));
  inf.on('end', () => console.log('inflate stream', Buffer.concat(chunks).equals(text)));
  const d = zlib.deflateSync(text);
  for (let i = 0; i < d.length; i += 7) inf.write(d.subarray(i, i + 7));
  inf.end();
}

const gz = zlib.createGzip();
const sizes = [];
gz.on('data', (c) => sizes.push(c.length));
gz.on('end', () => {
  console.log('gzip stream', sizes);
  setTimeout(inflateStream, 50);
});
gz.write(words.subarray(0, 5000));
gz.write(words.subarray(5000));
gz.end();
