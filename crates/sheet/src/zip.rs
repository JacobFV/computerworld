//! ZIP containers with DEFLATE (RFC 1950/1951), CRC-32 and all, in pure Rust: enough
//! to write Office Open XML and OpenDocument files byte-for-byte reproducibly and to
//! read the ones Excel, Numbers and LibreOffice write.

// ----- CRC-32 -----

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
const CRC: [u32; 256] = crc_table();
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c = CRC[((c ^ u32::from(*b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

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
            return Ok(out);
        }
    }
}

// ----- ZIP -----

/// MS-DOS date and time fields for a calendar moment.
pub fn dos_time(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> (u16, u16) {
    let year = year.clamp(1980, 2107) as u16;
    let date = ((year - 1980) << 9) | ((month as u16) << 5) | day as u16;
    let time = ((hour as u16) << 11) | ((minute as u16) << 5) | (second as u16 / 2);
    (date, time)
}
/// A ZIP archive of `(name, bytes)` entries, deflated, all stamped with `(date, time)`.
pub fn write(entries: &[(String, Vec<u8>)], stamp: (u16, u16)) -> Vec<u8> {
    write_with(entries, stamp, false)
}
/// Like [`write`], with the first entry stored uncompressed, as OpenDocument requires
/// of its `mimetype`.
pub fn write_first_stored(entries: &[(String, Vec<u8>)], stamp: (u16, u16)) -> Vec<u8> {
    write_with(entries, stamp, true)
}
fn write_with(entries: &[(String, Vec<u8>)], stamp: (u16, u16), store_first: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (i, (name, data)) in entries.iter().enumerate() {
        let crc = crc32(data);
        let packed = deflate(data);
        let (method, body) = if packed.len() < data.len() && !(store_first && i == 0) {
            (8u16, packed)
        } else {
            (0u16, data.clone())
        };
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes()); // made by
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // needed
            h.extend_from_slice(&0u16.to_le_bytes()); // flags
            h.extend_from_slice(&method.to_le_bytes());
            h.extend_from_slice(&stamp.1.to_le_bytes());
            h.extend_from_slice(&stamp.0.to_le_bytes());
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(body.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&0u16.to_le_bytes()); // comment
                h.extend_from_slice(&0u16.to_le_bytes()); // disk
                h.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(&body);
        central.extend(header(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend(central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "truncated archive".into())
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "truncated archive".into())
}
/// Every entry of a ZIP archive, decompressed and CRC-checked.
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // The end-of-central-directory record, searched for from the end past any comment.
    let min = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (min..=bytes.len().saturating_sub(22))
        .rev()
        .find(|i| bytes[*i..].starts_with(&0x0605_4b50u32.to_le_bytes()))
        .ok_or("not a ZIP archive")?;
    let count = usize::from(u16_at(bytes, eocd + 10)?);
    let mut at = u32_at(bytes, eocd + 16)? as usize;
    if count == 0xFFFF || at == 0xFFFF_FFFF {
        return Err("ZIP64 archives are not supported".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, at)? != 0x0201_4b50 {
            return Err("damaged central directory".into());
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let packed = u32_at(bytes, at + 20)? as usize;
        let size = u32_at(bytes, at + 24)? as usize;
        let name_len = usize::from(u16_at(bytes, at + 28)?);
        let extra_len = usize::from(u16_at(bytes, at + 30)?);
        let comment_len = usize::from(u16_at(bytes, at + 32)?);
        let local = u32_at(bytes, at + 42)? as usize;
        let name = String::from_utf8_lossy(
            bytes
                .get(at + 46..at + 46 + name_len)
                .ok_or("truncated archive")?,
        )
        .into_owned();
        at += 46 + name_len + extra_len + comment_len;
        if flags & 1 != 0 {
            return Err(format!("{name} is encrypted"));
        }
        if u32_at(bytes, local)? != 0x0403_4b50 {
            return Err("damaged local header".into());
        }
        let start = local
            + 30
            + usize::from(u16_at(bytes, local + 26)?)
            + usize::from(u16_at(bytes, local + 28)?);
        let body = bytes.get(start..start + packed).ok_or("truncated entry")?;
        let data = match method {
            0 => body.to_vec(),
            8 => inflate(body, size.max(1) * 2 + 1024)?,
            m => return Err(format!("{name} uses compression method {m}")),
        };
        if data.len() != size || crc32(&data) != crc {
            return Err(format!("{name} failed its integrity check"));
        }
        out.push((name, data));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
    #[test]
    fn deflate_round_trips_and_compresses_repetition() {
        let text = "<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c></row>".repeat(200);
        let packed = deflate(text.as_bytes());
        assert!(packed.len() < text.len() / 10, "{} bytes", packed.len());
        assert_eq!(inflate(&packed, 1 << 20).unwrap(), text.as_bytes());
        let noise: Vec<u8> = (0..5000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        assert_eq!(inflate(&deflate(&noise), 1 << 20).unwrap(), noise);
        assert_eq!(inflate(&deflate(b""), 10).unwrap(), b"");
    }
    #[test]
    fn inflate_reads_stored_and_dynamic_blocks() {
        // A stored block, and a dynamic-Huffman stream zlib produced at level 9.
        let stored = [1u8, 5, 0, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(inflate(&stored, 100).unwrap(), b"hello");
        let zlib_dynamic = [
            0xb5u8, 0xcb, 0xd1, 0x19, 0x80, 0x10, 0x18, 0x46, 0xe1, 0x55, 0xbe, 0xee, 0x7b, 0x5a,
            0x26, 0xb, 0x10, 0x49, 0xe1, 0x97, 0x10, 0xa6, 0xcf, 0x12, 0x5d, 0x9f, 0xf3, 0xb2,
            0x43, 0xe1, 0xce, 0x66, 0xbb, 0x20, 0x22, 0xbd, 0x1e, 0x3b, 0x55, 0x9c, 0xd9, 0x85,
            0x7, 0x54, 0x54, 0x44, 0x1a, 0xd9, 0xf2, 0xde, 0x20, 0x49, 0x2f, 0x60, 0xbf, 0xcd,
            0x6b, 0x38, 0x8c, 0xaf, 0xa0, 0x1d, 0xc2, 0xf2, 0x41, 0xee, 0xcc, 0x63, 0xea, 0xf3,
            0x0, 0x52, 0x2b, 0xb8, 0x86, 0x42, 0xef, 0xf4, 0x1,
        ];
        let expected = [
            &b"The quick brown fox jumps over the lazy dog. ".repeat(3)[..],
            b"Sphinx of black quartz, judge my vow!",
        ]
        .concat();
        assert_eq!(inflate(&zlib_dynamic, 1000).unwrap(), expected);
    }
    #[test]
    fn archives_round_trip() {
        let files = vec![
            ("[Content_Types].xml".to_string(), b"<Types/>".to_vec()),
            (
                "xl/workbook.xml".to_string(),
                "<workbook>".repeat(50).into_bytes(),
            ),
        ];
        let zip = write(&files, dos_time(2026, 9, 17, 9, 0, 0));
        assert_eq!(read(&zip).unwrap(), files);
        assert_eq!(
            zip,
            write(&files, dos_time(2026, 9, 17, 9, 0, 0)),
            "deterministic"
        );
    }
}
