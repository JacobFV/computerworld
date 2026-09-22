//! zlib streams (RFC 1950 around RFC 1951 DEFLATE) in pure Rust, for GIMP's XCF tiles:
//! LZ77 over a 32 KiB window with fixed Huffman codes when writing, and every block
//! type when reading. Output is a pure function of the input, so a file written on one
//! target is byte-for-byte the file written on any other.

// ----- DEFLATE compression: LZ77 over a 32 KiB window, fixed Huffman codes -----

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}
impl BitWriter {
    fn bits(&mut self, value: u32, count: u32) {
        self.acc |= u64::from(value) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code, most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let mut rev = 0;
        for i in 0..len {
            if code & (1 << i) != 0 {
                rev |= 1 << (len - 1 - i);
            }
        }
        self.bits(rev, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}
fn literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.code(0x30 + sym, 8),
        144..=255 => w.code(0x190 + sym - 144, 9),
        256..=279 => w.code(sym - 256, 7),
        _ => w.code(0xC0 + sym - 280, 8),
    }
}
/// Raw DEFLATE data (no zlib wrapper).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32_768;
    const HASH: usize = 1 << 15;
    const CHAIN: usize = 64;
    let mut w = BitWriter {
        out: Vec::new(),
        acc: 0,
        n: 0,
    };
    w.bits(1, 1); // final block
    w.bits(1, 2); // fixed Huffman codes
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |i: usize| -> usize {
        let v = u32::from(data[i]) << 16 | u32::from(data[i + 1]) << 8 | u32::from(data[i + 2]);
        (v.wrapping_mul(2_654_435_761) >> 17) as usize & (HASH - 1)
    };
    let mut i = 0;
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + 2 < data.len() {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i;
        }
    };
    while i < data.len() {
        let mut best_len = 0;
        let mut best_dist = 0;
        if i + 2 < data.len() {
            let mut cand = head[hash(i)];
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW && chain < CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                chain += 1;
            }
        }
        if best_len >= 3 {
            let li = LEN_BASE
                .iter()
                .rposition(|b| usize::from(*b) <= best_len)
                .unwrap_or(0);
            literal(&mut w, 257 + li as u32);
            w.bits(
                (best_len - usize::from(LEN_BASE[li])) as u32,
                u32::from(LEN_EXTRA[li]),
            );
            let di = DIST_BASE
                .iter()
                .rposition(|b| usize::from(*b) <= best_dist)
                .unwrap_or(0);
            w.code(di as u32, 5);
            w.bits(
                (best_dist - usize::from(DIST_BASE[di])) as u32,
                u32::from(DIST_EXTRA[di]),
            );
            for k in 0..best_len {
                insert(&mut head, &mut prev, i + k);
            }
            i += best_len;
        } else {
            literal(&mut w, u32::from(data[i]));
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    literal(&mut w, 256);
    w.finish()
}

// ----- DEFLATE decompression (stored, fixed and dynamic blocks) -----

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    n: u32,
}
impl BitReader<'_> {
    fn bits(&mut self, count: u32) -> Result<u32, String> {
        while self.n < count {
            let byte = *self.data.get(self.pos).ok_or("deflate data ends early")?;
            self.pos += 1;
            self.acc |= u32::from(byte) << self.n;
            self.n += 8;
        }
        let v = self.acc & ((1u64 << count) - 1) as u32;
        self.acc >>= count;
        self.n -= count;
        Ok(v)
    }
}
/// Canonical Huffman decoding table: code-length counts and symbols in code order.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}
impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut count = [0u16; 16];
        for l in lengths {
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
        Ok(Self { count, symbol })
    }
    fn decode(&self, r: &mut BitReader) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code".into())
    }
}
fn inflate_block(
    r: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = usize::from(sym - 257);
                let len = usize::from(LEN_BASE[i]) + r.bits(u32::from(LEN_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(r)?);
                if d >= 30 {
                    return Err("invalid distance code".into());
                }
                let distance =
                    usize::from(DIST_BASE[d]) + r.bits(u32::from(DIST_EXTRA[d]))? as usize;
                if distance > out.len() {
                    return Err("distance reaches before the data".into());
                }
                let start = out.len() - distance;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return Err("invalid literal/length code".into()),
        }
        if out.len() > limit {
            return Err("inflated data exceeds its declared size".into());
        }
    }
}
/// Inflate raw DEFLATE data, refusing to produce more than `limit` bytes.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    inflate_len(data, limit).map(|(out, _)| out)
}
/// Inflate, also returning how many input bytes the DEFLATE data took.
fn inflate_len(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), String> {
    let mut r = BitReader {
        data,
        pos: 0,
        acc: 0,
        n: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = r.bits(1)?;
        match r.bits(2)? {
            0 => {
                r.acc = 0;
                r.n = 0;
                let b = data
                    .get(r.pos..r.pos + 4)
                    .ok_or("stored block header ends early")?;
                let len = usize::from(u16::from_le_bytes([b[0], b[1]]));
                let nlen = u16::from_le_bytes([b[2], b[3]]);
                if nlen != !(len as u16) {
                    return Err("stored block length check failed".into());
                }
                r.pos += 4;
                out.extend_from_slice(
                    data.get(r.pos..r.pos + len)
                        .ok_or("stored block ends early")?,
                );
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
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
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
                            let prev = *lengths.last().ok_or("repeat with no previous length")?;
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
                    return Err("code lengths overflow".into());
                }
                let lit = Huffman::new(&lengths[..nlen])?;
                let dist = Huffman::new(&lengths[nlen..])?;
                inflate_block(&mut r, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err("invalid block type".into()),
        }
        if out.len() > limit {
            return Err("inflated data exceeds its declared size".into());
        }
        if last == 1 {
            return Ok((out, r.pos));
        }
    }
}

/// Adler-32 of `data`, the checksum a zlib stream ends with.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for v in chunk {
            a += u32::from(*v);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// A zlib stream: header (32 KiB window, default level), DEFLATE data, Adler-32.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9c];
    out.extend(deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Inflate a zlib stream, refusing more than `limit` bytes or a bad checksum.
pub fn decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let (cmf, flg) = match data {
        [a, b, ..] => (*a, *b),
        _ => return Err("zlib stream is too short".into()),
    };
    if cmf & 0x0f != 8 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err("not a zlib stream".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib preset dictionaries are not supported".into());
    }
    let (out, used) = inflate_len(&data[2..], limit)?;
    // The checksum follows the DEFLATE data; anything after it is not the stream's.
    let tail = data
        .get(2 + used..2 + used + 4)
        .ok_or("zlib stream has no checksum")?;
    let sum = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
    if sum != adler32(&out) {
        return Err("zlib checksum does not match".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zlib_round_trips_and_checks_its_sum() {
        let mut data = Vec::new();
        for i in 0..20_000u32 {
            data.push((i % 7) as u8);
            data.push((i * 31 % 251) as u8);
        }
        let z = compress(&data);
        assert!(z.len() < data.len() / 2, "{}", z.len());
        assert_eq!(decompress(&z, data.len()).unwrap(), data);
        assert!(decompress(&z, 10).is_err());
        let mut bad = z.clone();
        let n = bad.len();
        bad[n - 1] ^= 1;
        assert!(decompress(&bad, data.len()).is_err());
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(decompress(&compress(&[]), 0).unwrap(), Vec::<u8>::new());
        // Bytes after the stream (the next tile's) are not part of it.
        let mut padded = compress(b"abcabcabc");
        padded.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert_eq!(decompress(&padded, 64).unwrap(), b"abcabcabc");
    }
}
