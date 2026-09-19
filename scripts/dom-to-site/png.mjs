// Minimal PNG codec (8-bit RGB/RGBA/grey, non-interlaced) on node:zlib, so the pipeline
// needs no image dependency. Enough for Playwright screenshots and simulator frames.
import {deflateSync, inflateSync} from 'node:zlib';

const SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const CRC = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
function crc32(buf) {
  let c = -1;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
/// {width, height, rgba} -> PNG bytes.
export function encodePNG({width, height, rgba}) {
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    Buffer.from(rgba.buffer, rgba.byteOffset + y * stride, stride).copy(raw, y * (stride + 1) + 1);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([SIGNATURE, chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
/// PNG bytes -> {width, height, rgba: Uint8Array}.
export function decodePNG(buf) {
  if (!buf.subarray(0, 8).equals(SIGNATURE)) throw new Error('not a PNG');
  let at = 8, width = 0, height = 0, depth = 0, colour = 0, interlace = 0;
  const idat = [];
  while (at < buf.length) {
    const len = buf.readUInt32BE(at);
    const type = buf.toString('latin1', at + 4, at + 8);
    const data = buf.subarray(at + 8, at + 8 + len);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0); height = data.readUInt32BE(4);
      depth = data[8]; colour = data[9]; interlace = data[12];
    } else if (type === 'IDAT') idat.push(data);
    else if (type === 'IEND') break;
    at += 12 + len;
  }
  if (depth !== 8 || interlace !== 0) throw new Error(`unsupported PNG (depth ${depth}, interlace ${interlace})`);
  const channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour];
  if (!channels) throw new Error(`unsupported PNG colour type ${colour}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = new Uint8Array(width * height * 4);
  let prev = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = new Uint8Array(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? line[i - channels] : 0;
      const b = prev[i];
      const c = i >= channels ? prev[i - channels] : 0;
      let add = 0;
      if (filter === 1) add = a;
      else if (filter === 2) add = b;
      else if (filter === 3) add = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        add = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      line[i] = (line[i] + add) & 0xff;
    }
    for (let x = 0; x < width; x++) {
      const o = (y * width + x) * 4, s = x * channels;
      if (channels >= 3) { out[o] = line[s]; out[o + 1] = line[s + 1]; out[o + 2] = line[s + 2]; out[o + 3] = channels === 4 ? line[s + 3] : 255; }
      else { out[o] = out[o + 1] = out[o + 2] = line[s]; out[o + 3] = channels === 2 ? line[s + 1] : 255; }
    }
    prev = line;
  }
  return {width, height, rgba: out};
}
/// Mean colour of a rectangle, as [r, g, b]; null when the rectangle is empty.
export function meanColour(img, x, y, w, h) {
  const x0 = Math.max(0, Math.floor(x)), y0 = Math.max(0, Math.floor(y));
  const x1 = Math.min(img.width, Math.ceil(x + w)), y1 = Math.min(img.height, Math.ceil(y + h));
  if (x1 <= x0 || y1 <= y0) return null;
  let r = 0, g = 0, b = 0, n = 0;
  for (let yy = y0; yy < y1; yy++) for (let xx = x0; xx < x1; xx++) {
    const o = (yy * img.width + xx) * 4;
    r += img.rgba[o]; g += img.rgba[o + 1]; b += img.rgba[o + 2]; n++;
  }
  return [Math.round(r / n), Math.round(g / n), Math.round(b / n)];
}
