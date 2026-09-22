//! A minimal PNG decoder, so the engine (and the test harness) can turn `data:image/png`
//! URLs into pixels without the browser's image pipeline: every colour type at bit
//! depths 1 to 16, `tRNS` transparency, the five scanline filters, Adam7 interlacing,
//! and an inflate for zlib streams (stored, fixed and dynamic blocks). Output is
//! straight (non-premultiplied) 8-bit RGBA; 16-bit samples keep their high byte, as
//! `png::Transformations::STRIP_16` does in the browser. No dependencies, no I/O.

use super::RgbaImage;

/// Refuses images whose decoded RGBA would exceed this (64 MB).
const MAX_IMAGE_BYTES: u64 = 64 << 20;

// ---------------------------------------------------------------------------------
// Inflate (RFC 1951)
// ---------------------------------------------------------------------------------

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    n: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, count: u32) -> Option<u32> {
        while self.n < count {
            let byte = *self.data.get(self.pos)?;
            self.pos += 1;
            self.acc |= u32::from(byte) << self.n;
            self.n += 8;
        }
        let v = self.acc & ((1u64 << count) - 1) as u32;
        self.acc >>= count;
        self.n -= count;
        Some(v)
    }
}

/// Canonical Huffman decoding table: code-length counts and symbols in code order.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Huffman> {
        let mut count = [0u16; 16];
        for l in lengths {
            if *l > 15 {
                return None;
            }
            count[*l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + count[i - 1];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, l) in lengths.iter().enumerate() {
            if *l != 0 {
                symbol[offs[*l as usize] as usize] = s as u16;
                offs[*l as usize] += 1;
            }
        }
        Some(Huffman { count, symbol })
    }

    fn decode(&self, r: &mut BitReader) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

fn inflate_block(r: &mut BitReader, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, limit: usize) -> Option<()> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Some(()),
            257..=285 => {
                let i = usize::from(sym - 257);
                let len = usize::from(LEN_BASE[i]) + r.bits(u32::from(LEN_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(r)?);
                if d >= 30 {
                    return None;
                }
                let distance = usize::from(DIST_BASE[d]) + r.bits(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() || distance == 0 {
                    return None;
                }
                let start = out.len() - distance;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return None,
        }
        if out.len() > limit {
            return None;
        }
    }
}

/// Inflates raw DEFLATE data, refusing to produce more than `limit` bytes.
pub fn inflate(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut r = BitReader { data, pos: 0, acc: 0, n: 0 };
    let mut out = Vec::new();
    loop {
        let last = r.bits(1)?;
        match r.bits(2)? {
            0 => {
                r.acc = 0;
                r.n = 0;
                let b = data.get(r.pos..r.pos + 4)?;
                let len = usize::from(u16::from_le_bytes([b[0], b[1]]));
                let nlen = u16::from_le_bytes([b[2], b[3]]);
                if nlen != !(len as u16) {
                    return None;
                }
                r.pos += 4;
                out.extend_from_slice(data.get(r.pos..r.pos + len)?);
                r.pos += len;
            }
            1 => {
                let mut lengths = [0u8; 288];
                for (i, l) in lengths.iter_mut().enumerate() {
                    *l = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                let lit = Huffman::new(&lengths)?;
                let dist = Huffman::new(&[5u8; 30])?;
                inflate_block(&mut r, &mut out, &lit, &dist, limit)?;
            }
            2 => {
                let nlen = r.bits(5)? as usize + 257;
                let ndist = r.bits(5)? as usize + 1;
                let ncode = r.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut code_lengths = [0u8; 19];
                for &o in ORDER.iter().take(ncode) {
                    code_lengths[o] = r.bits(3)? as u8;
                }
                let code = Huffman::new(&code_lengths)?;
                let mut lengths = Vec::with_capacity(nlen + ndist);
                while lengths.len() < nlen + ndist {
                    let sym = code.decode(&mut r)?;
                    match sym {
                        0..=15 => lengths.push(sym as u8),
                        16 => {
                            let prev = *lengths.last()?;
                            for _ in 0..3 + r.bits(2)? {
                                lengths.push(prev);
                            }
                        }
                        17 => {
                            let run = 3 + r.bits(3)? as usize;
                            lengths.resize(lengths.len() + run, 0);
                        }
                        _ => {
                            let run = 11 + r.bits(7)? as usize;
                            lengths.resize(lengths.len() + run, 0);
                        }
                    }
                }
                if lengths.len() > nlen + ndist {
                    return None;
                }
                let lit = Huffman::new(&lengths[..nlen])?;
                let dist = Huffman::new(&lengths[nlen..])?;
                inflate_block(&mut r, &mut out, &lit, &dist, limit)?;
            }
            _ => return None,
        }
        if out.len() > limit {
            return None;
        }
        if last == 1 {
            return Some(out);
        }
    }
}

/// Inflates a zlib stream (RFC 1950): a two-byte header, DEFLATE data, an Adler-32
/// that is not checked (a corrupt image decodes to whatever it decodes to, as
/// browsers do, rather than to nothing).
pub fn inflate_zlib(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    if data.len() < 2 || data[0] & 0x0f != 8 || (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 != 0 {
        return None;
    }
    inflate(&data[2..], limit)
}

// ---------------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------------

fn be32(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlaced: bool,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 0,
        }
    }
    fn bits_per_pixel(&self) -> usize {
        self.channels() * self.depth as usize
    }
    fn bytes_per_pixel(&self) -> usize {
        self.bits_per_pixel().div_ceil(8).max(1)
    }
    fn row_bytes(&self, width: u32) -> usize {
        (width as usize * self.bits_per_pixel()).div_ceil(8)
    }
}

/// Is this a PNG (the eight-byte signature)?
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

/// Decodes a PNG to straight 8-bit RGBA. `None` for anything malformed or
/// unsupported.
pub fn decode_png(bytes: &[u8]) -> Option<RgbaImage> {
    if !is_png(bytes) {
        return None;
    }
    let mut pos = 8;
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while pos + 8 <= bytes.len() {
        let len = be32(bytes, pos)? as usize;
        let kind = bytes.get(pos + 4..pos + 8)?;
        let data = bytes.get(pos + 8..pos + 8 + len)?;
        match kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return None;
                }
                let h = Header {
                    width: be32(data, 0)?,
                    height: be32(data, 4)?,
                    depth: data[8],
                    color: data[9],
                    interlaced: data[12] == 1,
                };
                if h.width == 0 || h.height == 0 || data[10] != 0 || data[11] != 0 || data[12] > 1 {
                    return None;
                }
                let depth_ok = match h.color {
                    0 => matches!(h.depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(h.depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(h.depth, 8 | 16),
                    _ => false,
                };
                if !depth_ok || u64::from(h.width) * u64::from(h.height) * 4 > MAX_IMAGE_BYTES {
                    return None;
                }
                header = Some(h);
            }
            b"PLTE" => palette = data.as_chunks::<3>().0.to_vec(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        // Skip the CRC; a wrong one decodes like a right one.
        pos += 12 + len;
    }
    let h = header?;
    let (w, ht) = (h.width as usize, h.height as usize);
    let bpp = h.bytes_per_pixel();
    let expected = if h.interlaced {
        adam7_passes(h.width, h.height).iter().map(|&(pw, ph, ..)| if pw == 0 || ph == 0 { 0 } else { (h.row_bytes(pw) + 1) * ph as usize }).sum()
    } else {
        (h.row_bytes(h.width) + 1) * ht
    };
    let raw = inflate_zlib(&idat, expected)?;
    if raw.len() < expected {
        return None;
    }
    // Unfiltered scanlines of the whole image, packed at the source bit depth.
    let stride = h.row_bytes(h.width);
    let mut pixels = vec![0u8; stride * ht];
    if h.interlaced {
        let mut at = 0;
        for &(pw, ph, x0, y0, dx, dy) in &adam7_passes(h.width, h.height) {
            if pw == 0 || ph == 0 {
                continue;
            }
            let prow = h.row_bytes(pw);
            let mut rows = vec![0u8; prow * ph as usize];
            unfilter(&raw[at..at + (prow + 1) * ph as usize], &mut rows, prow, bpp)?;
            at += (prow + 1) * ph as usize;
            for py in 0..ph as usize {
                let y = y0 as usize + py * dy as usize;
                for px in 0..pw as usize {
                    let x = x0 as usize + px * dx as usize;
                    copy_sample(&h, &rows[py * prow..(py + 1) * prow], px, &mut pixels[y * stride..(y + 1) * stride], x);
                }
            }
        }
    } else {
        unfilter(&raw[..expected], &mut pixels, stride, bpp)?;
    }
    // Expand to RGBA.
    let mut rgba = Vec::with_capacity(w * ht * 4);
    let max = (1u32 << h.depth) - 1;
    let scale = |v: u32| -> u8 {
        if h.depth == 16 {
            (v >> 8) as u8
        } else if h.depth == 8 {
            v as u8
        } else {
            (v * 255 / max) as u8
        }
    };
    for y in 0..ht {
        let row = &pixels[y * stride..(y + 1) * stride];
        for x in 0..w {
            match h.color {
                0 => {
                    let v = sample(row, x, h.depth);
                    let a = if trns.len() >= 2 && v == u32::from(u16::from_be_bytes([trns[0], trns[1]])) { 0 } else { 255 };
                    let g = scale(v);
                    rgba.extend_from_slice(&[g, g, g, a]);
                }
                2 => {
                    let r = sample(row, x * 3, h.depth);
                    let g = sample(row, x * 3 + 1, h.depth);
                    let b = sample(row, x * 3 + 2, h.depth);
                    let transparent = trns.len() >= 6
                        && r == u32::from(u16::from_be_bytes([trns[0], trns[1]]))
                        && g == u32::from(u16::from_be_bytes([trns[2], trns[3]]))
                        && b == u32::from(u16::from_be_bytes([trns[4], trns[5]]));
                    rgba.extend_from_slice(&[scale(r), scale(g), scale(b), if transparent { 0 } else { 255 }]);
                }
                3 => {
                    let i = sample(row, x, h.depth) as usize;
                    let p = palette.get(i).copied().unwrap_or([0, 0, 0]);
                    let a = trns.get(i).copied().unwrap_or(255);
                    rgba.extend_from_slice(&[p[0], p[1], p[2], a]);
                }
                4 => {
                    let g = scale(sample(row, x * 2, h.depth));
                    let a = scale(sample(row, x * 2 + 1, h.depth));
                    rgba.extend_from_slice(&[g, g, g, a]);
                }
                _ => {
                    let r = scale(sample(row, x * 4, h.depth));
                    let g = scale(sample(row, x * 4 + 1, h.depth));
                    let b = scale(sample(row, x * 4 + 2, h.depth));
                    let a = scale(sample(row, x * 4 + 3, h.depth));
                    rgba.extend_from_slice(&[r, g, b, a]);
                }
            }
        }
    }
    Some(RgbaImage { width: h.width, height: h.height, rgba })
}

/// The `i`th sample of a packed row at `depth` bits.
fn sample(row: &[u8], i: usize, depth: u8) -> u32 {
    match depth {
        16 => u32::from(u16::from_be_bytes([row[i * 2], row[i * 2 + 1]])),
        8 => u32::from(row[i]),
        d => {
            let per_byte = 8 / d as usize;
            let byte = row[i / per_byte];
            let shift = 8 - d as usize * (i % per_byte + 1);
            u32::from(byte >> shift) & ((1 << d) - 1)
        }
    }
}

/// Copies pixel `sx` of a pass row into pixel `dx` of an image row (packed samples).
fn copy_sample(h: &Header, src: &[u8], sx: usize, dst: &mut [u8], dx: usize) {
    let bits = h.bits_per_pixel();
    if bits >= 8 {
        let n = bits / 8;
        dst[dx * n..dx * n + n].copy_from_slice(&src[sx * n..sx * n + n]);
    } else {
        let per_byte = 8 / bits;
        let v = (src[sx / per_byte] >> (8 - bits * (sx % per_byte + 1))) & ((1 << bits) - 1) as u8;
        let shift = 8 - bits * (dx % per_byte + 1);
        let b = &mut dst[dx / per_byte];
        *b = (*b & !(((1 << bits) - 1) as u8) << shift) | (v << shift);
    }
}

/// `(width, height, x0, y0, dx, dy)` of the seven Adam7 passes.
fn adam7_passes(w: u32, h: u32) -> [(u32, u32, u32, u32, u32, u32); 7] {
    const P: [(u32, u32, u32, u32); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
    let mut out = [(0, 0, 0, 0, 0, 0); 7];
    for (i, &(x0, y0, dx, dy)) in P.iter().enumerate() {
        let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
        let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
        out[i] = (pw, ph, x0, y0, dx, dy);
    }
    out
}

/// Undoes the per-scanline filters of `raw` (`rows` rows of `stride + 1` bytes) into
/// `out` (`stride` bytes per row).
fn unfilter(raw: &[u8], out: &mut [u8], stride: usize, bpp: usize) -> Option<()> {
    let rows = out.len() / stride.max(1);
    for y in 0..rows {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (before, cur) = out.split_at_mut(y * stride);
        let prev: &[u8] = if y == 0 { &[] } else { &before[(y - 1) * stride..] };
        let cur = &mut cur[..stride];
        for x in 0..stride {
            let a = if x >= bpp { cur[x - bpp] } else { 0 };
            let b = if y > 0 { prev[x] } else { 0 };
            let c = if y > 0 && x >= bpp { prev[x - bpp] } else { 0 };
            cur[x] = match filter {
                0 => line[x],
                1 => line[x].wrapping_add(a),
                2 => line[x].wrapping_add(b),
                3 => line[x].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => line[x].wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
        }
    }
    Some(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::data_url;

    // Acid2's 1x1 yellow pixel, its 2x2 yellow/transparent tile (RGB with tRNS) and
    // its 64x64 red square (`data:` URLs with percent-encoded base64).
    const YELLOW_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR42mP4%2F58BAAT%2FAf9jgNErAAAAAElFTkSuQmCC";
    const TILE_2X2: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAABnRSTlMAAAAAAABupgeRAAAABmJLR0QA%2FwD%2FAP%2BgvaeTAAAAEUlEQVR42mP4%2F58BCv7%2FZwAAHfAD%2FabwPj4AAAAASUVORK5CYII%3D";
    const RED_64: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAFSDNYfAAAAaklEQVR42u3XQQrAIAwAQeP%2F%2F6wf8CJBJTK9lnQ7FpHGaOurt1I34nfH9pMMZAZ8BwMGEvvh%2BBsJCAgICLwIOA8EBAQEBAQEBAQEBK79H5RfIQAAAAAAAAAAAAAAAAAAAAAAAAAAAID%2FABMSqAfj%2FsLmvAAAAABJRU5ErkJggg%3D%3D";

    fn decode(url: &str) -> RgbaImage {
        let (mime, bytes) = data_url::decode(url).expect("data url");
        assert_eq!(mime, "image/png");
        decode_png(&bytes).expect("png")
    }

    #[test]
    fn yellow_pixel() {
        let img = decode(YELLOW_1X1);
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.rgba, vec![255, 255, 0, 255]);
    }

    #[test]
    fn tile_with_trns_transparency() {
        let img = decode(TILE_2X2);
        assert_eq!((img.width, img.height), (2, 2));
        // Top-left and bottom-right yellow, the other two transparent.
        assert_eq!(&img.rgba[0..4], &[255, 255, 0, 255]);
        assert_eq!(img.rgba[7], 0);
        assert_eq!(img.rgba[11], 0);
        assert_eq!(&img.rgba[12..16], &[255, 255, 0, 255]);
    }

    #[test]
    fn red_square() {
        let img = decode(RED_64);
        assert_eq!((img.width, img.height), (64, 64));
        assert!(img.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_png(b"not a png").is_none());
        assert!(decode_png(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0]).is_none());
    }

    #[test]
    fn stored_block_inflates() {
        // zlib header, one stored final block of "abc".
        let z = [0x78, 0x01, 0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c', 0, 0, 0, 0];
        assert_eq!(inflate_zlib(&z, 100).unwrap(), b"abc");
    }
}
