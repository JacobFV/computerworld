//! Archive and sync commands: `tar`, `gzip`/`gunzip`/`zcat`, `zip`/`unzip`, `rsync`.
//! Real container bytes, so an archive written here unpacks on a host and back.
//!
//! Nothing here is a stub format. `tar` writes ustar headers with a correct checksum
//! and the prefix/name split; `gzip` writes an RFC 1952 member around an RFC 1951
//! DEFLATE stream; `zip` writes PKZIP local records, a central directory and an
//! end-of-central-directory. Every timestamp comes from the world clock, never the
//! host, so the same world replays the same archive bytes.
//!
//! Compression is deliberately *stored*: DEFLATE `BTYPE=00` blocks and ZIP method 0.
//! That is a legitimate, fully interoperable encoding — a host `gunzip` or `unzip`
//! reads it without complaint — and it is stated plainly rather than dressed up as
//! compression this module does not perform. Decompression is the real thing: a
//! complete inflate covering stored, fixed-Huffman and dynamic-Huffman blocks with
//! LZ77 back-references, so a `.gz` or a deflated `.zip` member produced by a host
//! tool really unpacks here.
use crate::shell::{flag, mode_string, options, value, walk_tree, Fail};
use crate::Computer;

// ---------------------------------------------------------------------------
// World clock
// ---------------------------------------------------------------------------

/// Unix seconds for a world tick. The origin is `shell::EPOCH_UNIX_SECONDS`; no host
/// clock is consulted anywhere in this module.
fn unix_seconds(tick: u64) -> u64 {
    crate::shell::EPOCH_UNIX_SECONDS + tick / 1_000_000
}
/// The inverse: an archived timestamp becomes the tick the VFS stores. Anything from
/// before the world began lands on tick 0 rather than wrapping.
fn tick_of(unix: u64) -> u64 {
    unix.saturating_sub(crate::shell::EPOCH_UNIX_SECONDS)
        .saturating_mul(1_000_000)
}
/// `2026-09-17 09:00`, the stamp `tar -tv` and `unzip -l` print.
fn stamp(tick: u64) -> String {
    let k = crate::shell::clock(tick);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        k.year, k.month, k.day, k.hour, k.minute
    )
}
/// Days since the Unix epoch for a civil date (Howard Hinnant's `days_from_civil`),
/// used to turn a DOS date back into a tick.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// CRC-32 (RFC 1952 / PKZIP)
// ---------------------------------------------------------------------------

const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}
static CRC32_TABLE: [u32; 256] = crc32_table();
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in bytes {
        c = CRC32_TABLE[((c ^ u32::from(*b)) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

// ---------------------------------------------------------------------------
// DEFLATE (RFC 1951)
// ---------------------------------------------------------------------------

/// LSB-first bit reader over a DEFLATE stream.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self
            .data
            .get(self.pos / 8)
            .ok_or_else(|| "unexpected end of the compressed stream".to_string())?;
        let value = u32::from(byte >> (self.pos % 8)) & 1;
        self.pos += 1;
        Ok(value)
    }
    fn bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..count {
            value |= self.bit()? << i;
        }
        Ok(value)
    }
    fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
    }
    fn byte_pos(&self) -> usize {
        self.pos / 8
    }
}

/// A canonical Huffman decoder held as zlib's `puff` holds it: a count per code
/// length, and the symbols ordered by (length, symbol).
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}
impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err("a Huffman code length exceeds 15 bits".into());
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        // An incomplete code is legal (the fixed distance table has 30 of 32 codes);
        // an over-subscribed one is not.
        let mut left = 1i32;
        for count in counts.iter().skip(1) {
            left = (left << 1) - i32::from(*count);
            if left < 0 {
                return Err("over-subscribed Huffman code".into());
            }
        }
        let mut offsets = [0u16; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = symbol as u16;
                offsets[l as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }
    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.bit()? as i32;
            let count = i32::from(self.counts[len]);
            if code - first < count {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("invalid Huffman code in the compressed stream".into())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
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
/// The order RFC 1951 §3.2.7 stores the code-length code lengths in.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The literal/length and distance tables of a `BTYPE=01` block, RFC 1951 §3.2.6.
fn fixed_tables() -> (Huffman, Huffman) {
    let mut literal = [0u8; 288];
    for (symbol, length) in literal.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let expect = "the RFC 1951 fixed tables are well formed";
    (
        Huffman::new(&literal).expect(expect),
        Huffman::new(&[5u8; 30]).expect(expect),
    )
}

/// The tables of a `BTYPE=10` block: a code-length alphabet, then the literal and
/// distance lengths it encodes, with the 16/17/18 repeat codes expanded.
fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), String> {
    let hlit = bits.bits(5)? as usize + 257;
    let hdist = bits.bits(5)? as usize + 1;
    let hclen = bits.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err("a dynamic block declares more codes than RFC 1951 allows".into());
    }
    let mut code_lengths = [0u8; 19];
    for &slot in CODE_LENGTH_ORDER.iter().take(hclen) {
        code_lengths[slot] = bits.bits(3)? as u8;
    }
    let alphabet = Huffman::new(&code_lengths)?;
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < lengths.len() {
        let symbol = alphabet.decode(bits)?;
        let (repeat, value) = match symbol {
            0..=15 => {
                lengths[i] = symbol as u8;
                i += 1;
                continue;
            }
            16 => {
                if i == 0 {
                    return Err("a code-length repeat has nothing to repeat".into());
                }
                (3 + bits.bits(2)? as usize, lengths[i - 1])
            }
            17 => (3 + bits.bits(3)? as usize, 0),
            18 => (11 + bits.bits(7)? as usize, 0),
            _ => return Err("invalid code-length symbol".into()),
        };
        if i + repeat > lengths.len() {
            return Err("a code-length repeat overruns the table".into());
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    Ok((
        Huffman::new(&lengths[..hlit])?,
        Huffman::new(&lengths[hlit..])?,
    ))
}

/// Real RFC 1951 inflate. Returns the decompressed bytes and the number of whole
/// bytes consumed, so a gzip trailer or the next ZIP record can be found.
fn inflate(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut bits = Bits::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bit()? == 1;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let p = bits.byte_pos();
                if p + 4 > data.len() {
                    return Err("truncated stored block".into());
                }
                let len = usize::from(u16::from_le_bytes([data[p], data[p + 1]]));
                let nlen = usize::from(u16::from_le_bytes([data[p + 2], data[p + 3]]));
                if len ^ 0xffff != nlen {
                    return Err("stored block length check failed".into());
                }
                let end = p + 4 + len;
                if end > data.len() {
                    return Err("truncated stored block".into());
                }
                out.extend_from_slice(&data[p + 4..end]);
                bits.pos = end * 8;
            }
            kind @ (1 | 2) => {
                let (literals, distances) = if kind == 1 {
                    fixed_tables()
                } else {
                    dynamic_tables(&mut bits)?
                };
                loop {
                    let symbol = literals.decode(&mut bits)?;
                    if symbol < 256 {
                        out.push(symbol as u8);
                        continue;
                    }
                    if symbol == 256 {
                        break;
                    }
                    let i = usize::from(symbol) - 257;
                    if i >= LENGTH_BASE.len() {
                        return Err("invalid length code".into());
                    }
                    let length = usize::from(LENGTH_BASE[i])
                        + bits.bits(u32::from(LENGTH_EXTRA[i]))? as usize;
                    let d = usize::from(distances.decode(&mut bits)?);
                    if d >= DIST_BASE.len() {
                        return Err("invalid distance code".into());
                    }
                    let distance =
                        usize::from(DIST_BASE[d]) + bits.bits(u32::from(DIST_EXTRA[d]))? as usize;
                    if distance == 0 || distance > out.len() {
                        return Err("a back-reference points before the start of the stream".into());
                    }
                    // Overlapping copies are legal and are how runs are encoded, so
                    // the bytes are appended one at a time.
                    let from = out.len() - distance;
                    for k in 0..length {
                        let byte = out[from + k];
                        out.push(byte);
                    }
                }
            }
            _ => return Err("reserved DEFLATE block type".into()),
        }
        if last {
            break;
        }
    }
    bits.align();
    Ok((out, bits.byte_pos()))
}

/// DEFLATE as *stored* blocks: `BTYPE=00`, no back-reference search. Interoperable
/// with every inflater; honest about performing no compression.
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![1, 0, 0, 0xff, 0xff];
    }
    let mut out = Vec::with_capacity(data.len() + data.len() / 0xffff * 5 + 5);
    let mut chunks = data.chunks(0xffff).peekable();
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

// ---------------------------------------------------------------------------
// gzip container (RFC 1952)
// ---------------------------------------------------------------------------

/// One gzip member: magic, CM=8 deflate, no flags, the world-clock MTIME, XFL=0,
/// OS=3 (Unix), the DEFLATE stream, then CRC32 and ISIZE little-endian.
fn gzip_member(data: &[u8], mtime: u32) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0x00];
    out.extend_from_slice(&mtime.to_le_bytes());
    out.push(0x00);
    out.push(0x03);
    out.extend_from_slice(&deflate_stored(data));
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// Decode one member; returns the bytes, the stored MTIME and the offset past the
/// trailer. Both trailer fields are checked and a mismatch is a loud failure.
fn gunzip_member(data: &[u8]) -> Result<(Vec<u8>, u32, usize), String> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b {
        return Err("not in gzip format".into());
    }
    if data[2] != 8 {
        return Err(format!("unknown compression method {}", data[2]));
    }
    let flags = data[3];
    if flags & 0xe0 != 0 {
        return Err("reserved gzip flag bits are set".into());
    }
    let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let mut p = 10;
    if flags & 0x04 != 0 {
        if p + 2 > data.len() {
            return Err("truncated gzip extra field".into());
        }
        p += 2 + usize::from(u16::from_le_bytes([data[p], data[p + 1]]));
    }
    for bit in [0x08u8, 0x10] {
        if flags & bit != 0 {
            let rest = data
                .get(p..)
                .ok_or_else(|| "truncated gzip header".to_string())?;
            let end = rest
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| "truncated gzip header string".to_string())?;
            p += end + 1;
        }
    }
    if flags & 0x02 != 0 {
        p += 2; // header CRC16: the whole-member CRC32 below is the real check.
    }
    if p >= data.len() {
        return Err("truncated gzip header".into());
    }
    let (out, used) = inflate(&data[p..])?;
    let end = p + used;
    if end + 8 > data.len() {
        return Err("truncated gzip trailer".into());
    }
    let stored_crc = u32::from_le_bytes([data[end], data[end + 1], data[end + 2], data[end + 3]]);
    let stored_size =
        u32::from_le_bytes([data[end + 4], data[end + 5], data[end + 6], data[end + 7]]);
    if stored_crc != crc32(&out) {
        return Err("CRC check failed".into());
    }
    if stored_size != out.len() as u32 {
        return Err("length check failed".into());
    }
    Ok((out, mtime, end + 8))
}

/// A whole gzip file: one or more members, as `cat a.gz b.gz` produces.
fn gunzip_bytes(data: &[u8]) -> Result<(Vec<u8>, u32), String> {
    let (mut out, mut mtime, mut p, mut members) = (Vec::new(), 0u32, 0usize, 0usize);
    while p < data.len() {
        // Some writers pad the tail with zeros; that is not another member.
        if data[p..].iter().all(|b| *b == 0) {
            break;
        }
        let (bytes, member_mtime, used) = gunzip_member(&data[p..])?;
        if members == 0 {
            mtime = member_mtime;
        }
        out.extend_from_slice(&bytes);
        p += used;
        members += 1;
    }
    if members == 0 {
        return Err("not in gzip format".into());
    }
    Ok((out, mtime))
}

// ---------------------------------------------------------------------------
// ustar
// ---------------------------------------------------------------------------

const BLOCK: usize = 512;

/// One archive member, shared by the tar writer and reader.
struct Member {
    /// The name as stored: directories carry a trailing `/`.
    name: String,
    mode: u16,
    size: u64,
    /// Unix seconds, as the header stores them.
    mtime: u64,
    /// ustar typeflag: `0` regular, `5` directory, `2` symbolic link.
    kind: u8,
    link: String,
    uname: String,
    gname: String,
    data: Vec<u8>,
}
impl Member {
    fn is_dir(&self) -> bool {
        self.kind == b'5'
    }
    fn is_symlink(&self) -> bool {
        self.kind == b'2'
    }
}

/// Numeric ids without a user database: root is 0, everyone else is the first
/// ordinary account, exactly as the rest of this world models ownership.
fn ids(owner: &str) -> (u64, u64) {
    if owner == "root" {
        (0, 0)
    } else {
        (1000, 1000)
    }
}

/// A ustar numeric field: zero-padded octal, NUL terminated.
fn octal(field: &mut [u8], value: u64) -> Result<(), Fail> {
    let width = field.len() - 1;
    let text = format!("{value:0width$o}");
    if text.len() > width {
        return Err(Fail::new(
            format!("tar: {value}: value does not fit a ustar {width}-digit field"),
            1,
        ));
    }
    field[..width].copy_from_slice(text.as_bytes());
    field[width] = 0;
    Ok(())
}
fn parse_octal(field: &[u8]) -> u64 {
    field
        .iter()
        .copied()
        .skip_while(|b| *b == b' ' || *b == 0)
        .take_while(|b| b.is_ascii_digit() && *b < b'8')
        .fold(0u64, |acc, b| acc * 8 + u64::from(b - b'0'))
}
fn cstr(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// Split a member name over the ustar `prefix` and `name` fields. The split must fall
/// on a `/`; the shortest prefix that leaves a fitting name wins. `None` means the
/// name cannot be represented and the caller refuses it by name.
fn split_name(name: &str) -> Option<(&str, &str)> {
    if name.len() <= 100 {
        return Some(("", name));
    }
    name.match_indices('/')
        .find(|(i, _)| *i <= 155 && name.len() - i - 1 <= 100 && *i > 0)
        .map(|(i, _)| (&name[..i], &name[i + 1..]))
}

fn ustar_header(m: &Member) -> Result<[u8; BLOCK], Fail> {
    let mut h = [0u8; BLOCK];
    let (prefix, name) = split_name(&m.name).ok_or_else(|| {
        Fail::new(
            format!(
                "tar: {}: file name is too long for the ustar format",
                m.name
            ),
            1,
        )
    })?;
    if m.link.len() > 100 {
        return Err(Fail::new(
            format!(
                "tar: {}: link target is too long for the ustar format",
                m.name
            ),
            1,
        ));
    }
    let (uid, gid) = ids(&m.uname);
    h[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut h[100..108], u64::from(m.mode & 0o7777))?;
    octal(&mut h[108..116], uid)?;
    octal(&mut h[116..124], gid)?;
    octal(&mut h[124..136], m.size)?;
    octal(&mut h[136..148], m.mtime)?;
    h[156] = m.kind;
    h[157..157 + m.link.len()].copy_from_slice(m.link.as_bytes());
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let uname = m.uname.as_bytes();
    let gname = m.gname.as_bytes();
    h[265..265 + uname.len().min(31)].copy_from_slice(&uname[..uname.len().min(31)]);
    h[297..297 + gname.len().min(31)].copy_from_slice(&gname[..gname.len().min(31)]);
    octal(&mut h[329..337], 0)?;
    octal(&mut h[337..345], 0)?;
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    // The checksum is computed with its own field read as eight spaces.
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    Ok(h)
}

fn write_tar(members: &[Member]) -> Result<Vec<u8>, Fail> {
    let mut out = Vec::new();
    for m in members {
        out.extend_from_slice(&ustar_header(m)?);
        out.extend_from_slice(&m.data);
        let padding = m.data.len().div_ceil(BLOCK) * BLOCK - m.data.len();
        out.extend(std::iter::repeat_n(0u8, padding));
    }
    out.extend(std::iter::repeat_n(0u8, 2 * BLOCK));
    Ok(out)
}

fn read_tar(bytes: &[u8]) -> Result<Vec<Member>, Fail> {
    let bad = |what: &str| Fail::new(format!("tar: {what}"), 1);
    let mut out: Vec<Member> = Vec::new();
    let (mut p, mut long_name, mut long_link) = (0usize, None::<String>, None::<String>);
    while p + BLOCK <= bytes.len() {
        let h = &bytes[p..p + BLOCK];
        if h.iter().all(|b| *b == 0) {
            break;
        }
        let stored = parse_octal(&h[148..156]);
        let sum: u32 = h
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u32::from(*b)
                }
            })
            .sum();
        if u64::from(sum) != stored {
            return Err(bad("This does not look like a tar archive"));
        }
        let kind = h[156];
        let size = parse_octal(&h[124..136]) as usize;
        p += BLOCK;
        let end = p + size;
        if end > bytes.len() {
            return Err(bad("Unexpected EOF in archive"));
        }
        let data = bytes[p..end].to_vec();
        p += size.div_ceil(BLOCK) * BLOCK;
        // GNU long-name records carry the following member's name in their payload.
        if kind == b'L' || kind == b'K' {
            let text = cstr(&data);
            if kind == b'L' {
                long_name = Some(text);
            } else {
                long_link = Some(text);
            }
            continue;
        }
        let name = long_name.take().unwrap_or_else(|| {
            let (short, prefix) = (cstr(&h[..100]), cstr(&h[345..500]));
            if prefix.is_empty() {
                short
            } else {
                format!("{prefix}/{short}")
            }
        });
        let link = long_link.take().unwrap_or_else(|| cstr(&h[157..257]));
        let kind = match kind {
            b'0' | 0 | b'7' => b'0',
            b'5' => b'5',
            b'2' => b'2',
            other => {
                let shown = if other.is_ascii_graphic() {
                    (other as char).to_string()
                } else {
                    format!("\\{other:03o}")
                };
                return Err(bad(&format!(
                    "{name}: Unsupported entry type '{shown}'; refusing rather than dropping it"
                )));
            }
        };
        out.push(Member {
            name,
            mode: (parse_octal(&h[100..108]) & 0o7777) as u16,
            size: size as u64,
            mtime: parse_octal(&h[136..148]),
            kind,
            link,
            uname: cstr(&h[265..297]),
            gname: cstr(&h[297..329]),
            data,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// tar
// ---------------------------------------------------------------------------

/// `--strip-components` has no short spelling; this sentinel keeps it out of the
/// single-letter namespace while still flowing through the shared option parser.
const STRIP: char = '\u{1}';

/// `tar -c|-x|-t -f FILE [-v] [-z] [-C DIR] [--strip-components=N]`.
pub(crate) fn tar(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let valued = format!("fC{STRIP}");
    let (opts, operands) = options(
        "tar",
        args,
        "cxtvz",
        &valued,
        &[
            ("create", 'c'),
            ("extract", 'x'),
            ("get", 'x'),
            ("list", 't'),
            ("file", 'f'),
            ("verbose", 'v'),
            ("gzip", 'z'),
            ("gunzip", 'z'),
            ("ungzip", 'z'),
            ("directory", 'C'),
            ("strip-components", STRIP),
        ],
    )?;
    let modes: Vec<char> = ['c', 'x', 't']
        .into_iter()
        .filter(|m| flag(&opts, *m))
        .collect();
    if modes.len() != 1 {
        return Err(Fail::usage(
            "tar: exactly one of -c, -x or -t must be given",
        ));
    }
    let file = value(&opts, 'f').ok_or_else(|| {
        Fail::usage("tar: -f ARCHIVE is required: this world has no default tape device")
    })?;
    if file == "-" {
        return Err(Fail::usage(
            "tar: -f -: reading or writing the archive on a pipe is not supported",
        ));
    }
    let archive = c.resolve(file);
    let base = match value(&opts, 'C') {
        Some(dir) => c.resolve(dir),
        None => c.cwd.clone(),
    };
    let strip = match value(&opts, STRIP) {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| Fail::usage(format!("tar: --strip-components={v}: invalid number")))?,
        None => 0,
    };
    let (verbose, gz) = (flag(&opts, 'v'), flag(&opts, 'z'));
    match modes[0] {
        'c' => tar_create(c, &archive, &base, &operands, verbose, gz, t),
        _ => {
            let raw = c.vfs.read_as(&archive, &c.user.clone()).map_err(|_| {
                Fail::new(
                    format!("tar: {file}: Cannot open: No such file or directory"),
                    1,
                )
            })?;
            let bytes = if gz {
                gunzip_bytes(&raw)
                    .map_err(|e| Fail::new(format!("tar: {file}: {e}"), 1))?
                    .0
            } else {
                raw
            };
            let members = read_tar(&bytes)?;
            if modes[0] == 't' {
                Ok(tar_list(&members, verbose))
            } else {
                tar_extract(c, &members, &base, strip, verbose, t)
            }
        }
    }
}

/// The name a member is stored under: no leading `/`, no `./`, no trailing `/`.
fn member_name(operand: &str) -> String {
    let trimmed = operand.trim_start_matches('/').trim_end_matches('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        parts.push(part);
    }
    parts.join("/")
}

fn tar_create(
    c: &mut Computer,
    archive: &str,
    base: &str,
    operands: &[String],
    verbose: bool,
    gz: bool,
    t: u64,
) -> Result<String, Fail> {
    if operands.is_empty() {
        return Err(Fail::usage(
            "tar: cowardly refusing to create an empty archive",
        ));
    }
    let user = c.user.clone();
    match c.vfs.stat(base) {
        Ok(m) if m.is_dir => {}
        _ => {
            return Err(Fail::new(
                format!("tar: {base}: Cannot chdir: No such file or directory"),
                1,
            ))
        }
    }
    let mut members = Vec::new();
    let mut listing = String::new();
    for operand in operands {
        let name = member_name(operand);
        if name.is_empty() {
            return Err(Fail::usage(format!(
                "tar: {operand}: not a valid member name"
            )));
        }
        let root = crate::vfs::normalize_path(base, operand);
        if !c.vfs.exists(&root) && c.vfs.lstat(&root).is_err() {
            return Err(Fail::new(
                format!("tar: {operand}: Cannot stat: No such file or directory"),
                1,
            ));
        }
        for (path, _, _) in walk_tree(c, &root) {
            let suffix = path[root.len().min(path.len())..].trim_start_matches('/');
            let stored = if suffix.is_empty() {
                name.clone()
            } else {
                format!("{name}/{suffix}")
            };
            let meta = c.vfs.lstat(&path)?;
            let (kind, data, link) = if meta.is_symlink {
                (b'2', Vec::new(), c.vfs.read_link(&path)?)
            } else if meta.is_dir {
                (b'5', Vec::new(), String::new())
            } else {
                let bytes = c
                    .vfs
                    .read_as(&path, &user)
                    .map_err(|e| Fail::new(format!("tar: {stored}: Cannot read: {e}"), 1))?;
                (b'0', bytes, String::new())
            };
            let stored = if kind == b'5' {
                format!("{stored}/")
            } else {
                stored
            };
            if verbose {
                listing.push_str(&stored);
                listing.push('\n');
            }
            members.push(Member {
                name: stored,
                mode: meta.mode & 0o7777,
                size: data.len() as u64,
                mtime: unix_seconds(meta.modified),
                kind,
                link,
                uname: meta.owner.clone(),
                gname: meta.group.clone(),
                data,
            });
        }
    }
    let bytes = write_tar(&members)?;
    let bytes = if gz {
        gzip_member(&bytes, unix_seconds(t) as u32)
    } else {
        bytes
    };
    c.vfs
        .write_as(archive, &bytes, &user, t)
        .map_err(|e| Fail::new(format!("tar: {archive}: Cannot write: {e}"), 1))?;
    Ok(listing)
}

fn tar_list(members: &[Member], verbose: bool) -> String {
    if !verbose {
        return members
            .iter()
            .map(|m| format!("{}\n", m.name))
            .collect::<String>();
    }
    let width = members
        .iter()
        .map(|m| m.size.to_string().len())
        .max()
        .unwrap_or(1);
    let mut out = String::new();
    for m in members {
        let owner = format!("{}/{}", m.uname, m.gname);
        out.push_str(&format!(
            "{} {} {:>width$} {} {}",
            mode_string(m.mode, m.is_dir(), m.is_symlink()),
            owner,
            m.size,
            stamp(tick_of(m.mtime)),
            m.name
        ));
        if m.is_symlink() {
            out.push_str(&format!(" -> {}", m.link));
        }
        out.push('\n');
    }
    out
}

/// Drop `n` leading path components; `None` means the member had no more and is
/// skipped, which is what GNU tar does.
fn strip_components(name: &str, n: usize) -> Option<String> {
    let mut parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
    if n == 0 {
        return Some(parts.join("/"));
    }
    if parts.len() <= n {
        return None;
    }
    Some(parts.split_off(n).join("/"))
}

fn tar_extract(
    c: &mut Computer,
    members: &[Member],
    base: &str,
    strip: usize,
    verbose: bool,
    t: u64,
) -> Result<String, Fail> {
    let user = c.user.clone();
    let mut listing = String::new();
    // Directory metadata is applied last: writing a child moves its parent's mtime.
    let mut directories: Vec<(String, u16, u64)> = Vec::new();
    for m in members {
        if m.name.split('/').any(|p| p == "..") {
            return Err(Fail::new(
                format!("tar: {}: Contains '..'; member not extracted", m.name),
                1,
            ));
        }
        let Some(relative) = strip_components(&m.name, strip) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let path = crate::vfs::normalize_path(base, &relative);
        let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        c.vfs
            .mkdir_all_as(if parent.is_empty() { "/" } else { parent }, &user, t)
            .map_err(|e| Fail::new(format!("tar: {relative}: Cannot mkdir: {e}"), 1))?;
        if verbose {
            listing.push_str(&m.name);
            listing.push('\n');
        }
        if m.is_dir() {
            c.vfs
                .mkdir_all_as(&path, &user, t)
                .map_err(|e| Fail::new(format!("tar: {relative}: Cannot mkdir: {e}"), 1))?;
            directories.push((path, m.mode, tick_of(m.mtime)));
            continue;
        }
        if c.vfs.lstat(&path).is_ok() {
            c.vfs
                .remove_as(&path, false, &user)
                .map_err(|e| Fail::new(format!("tar: {relative}: Cannot unlink: {e}"), 1))?;
        }
        if m.is_symlink() {
            c.vfs
                .symlink_as(&m.link, &path, &user, tick_of(m.mtime))
                .map_err(|e| Fail::new(format!("tar: {relative}: Cannot symlink: {e}"), 1))?;
            continue;
        }
        c.vfs
            .write_as(&path, &m.data, &user, tick_of(m.mtime))
            .map_err(|e| Fail::new(format!("tar: {relative}: Cannot write: {e}"), 1))?;
        let _ = c.vfs.chmod_as(&path, m.mode, &user);
        let _ = c.vfs.set_times_as(
            &path,
            Some(tick_of(m.mtime)),
            Some(tick_of(m.mtime)),
            &user,
            t,
            true,
        );
    }
    directories.sort();
    for (path, mode, tick) in directories.into_iter().rev() {
        let _ = c.vfs.chmod_as(&path, mode, &user);
        let _ = c
            .vfs
            .set_times_as(&path, Some(tick), Some(tick), &user, t, true);
    }
    Ok(listing)
}

// ---------------------------------------------------------------------------
// gzip / gunzip / zcat
// ---------------------------------------------------------------------------

/// The suffix `gunzip` strips, and what it leaves behind.
fn strip_gz_suffix(name: &str) -> Option<String> {
    for (suffix, replacement) in [
        (".tar.gz", ".tar"),
        (".tgz", ".tar"),
        (".taz", ".tar"),
        (".gz", ""),
        (".z", ""),
        (".Z", ""),
        ("-gz", ""),
        ("-z", ""),
    ] {
        if let Some(stem) = name.strip_suffix(suffix) {
            if !stem.is_empty() {
                return Some(format!("{stem}{replacement}"));
            }
        }
    }
    None
}

/// `gzip [-k] [-c] [-d] [-f] [-n] [-v] [-1..-9] FILE...`; `gunzip` is `gzip -d` and
/// `zcat` is `gzip -dc`. Compressing to stdout is refused by name, because a
/// command's standard output is text here and the bytes could not survive it.
pub(crate) fn gzip(
    c: &mut Computer,
    cmd: &str,
    args: &[String],
    input: &str,
    t: u64,
) -> Result<String, Fail> {
    let (opts, operands) = options(
        cmd,
        args,
        "kcdfnv123456789",
        "",
        &[
            ("keep", 'k'),
            ("stdout", 'c'),
            ("to-stdout", 'c'),
            ("decompress", 'd'),
            ("uncompress", 'd'),
            ("force", 'f'),
            ("no-name", 'n'),
            ("verbose", 'v'),
            ("fast", '1'),
            ("best", '9'),
        ],
    )?;
    let decompress = flag(&opts, 'd') || cmd == "gunzip" || cmd == "zcat";
    let to_stdout = flag(&opts, 'c') || cmd == "zcat" || operands.is_empty();
    // A command's standard output is text in this shell, so compressed bytes cannot
    // survive it. Refusing is the honest answer; a corrupt `.gz` that only shows up
    // when someone tries to read it back is not.
    if !decompress && to_stdout {
        return Err(Fail::usage(format!(
            "{cmd}: cannot write compressed bytes to stdout: this shell's stdout is \
             text, so name an output file instead"
        )));
    }
    let (keep, force, verbose) = (flag(&opts, 'k'), flag(&opts, 'f'), flag(&opts, 'v'));
    let no_name = flag(&opts, 'n');
    let user = c.user.clone();
    if operands.is_empty() {
        // Only the decompressing direction reaches here; the other was refused above.
        let (out, _) = gunzip_bytes(input.as_bytes())
            .map_err(|e| Fail::new(format!("{cmd}: stdin: {e}"), 1))?;
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let mut out = String::new();
    for name in &operands {
        let path = c.resolve(name);
        let meta = c
            .vfs
            .lstat(&path)
            .map_err(|_| Fail::new(format!("{cmd}: {name}: No such file or directory"), 1))?;
        if meta.is_dir {
            return Err(Fail::new(format!("{cmd}: {name} is a directory"), 1));
        }
        let bytes = c
            .vfs
            .read_as(&path, &user)
            .map_err(|e| Fail::new(format!("{cmd}: {name}: {e}"), 1))?;
        let (result, target) = if decompress {
            let (data, _) =
                gunzip_bytes(&bytes).map_err(|e| Fail::new(format!("{cmd}: {name}: {e}"), 1))?;
            let target = strip_gz_suffix(name)
                .ok_or_else(|| Fail::new(format!("{cmd}: {name}: unknown suffix -- ignored"), 1))?;
            (data, target)
        } else {
            if strip_gz_suffix(name).is_some() {
                return Err(Fail::new(
                    format!("{cmd}: {name} already has .gz suffix -- unchanged"),
                    1,
                ));
            }
            // The stored MTIME is the file's own world-clock modification time.
            let mtime = if no_name {
                0
            } else {
                unix_seconds(meta.modified) as u32
            };
            (gzip_member(&bytes, mtime), format!("{name}.gz"))
        };
        if verbose {
            let ratio = if bytes.is_empty() {
                0.0
            } else {
                100.0 - (result.len() as f64) * 100.0 / (bytes.len() as f64)
            };
            let verb = if to_stdout {
                "written to stdout"
            } else {
                "replaced with"
            };
            let suffix = if to_stdout {
                String::new()
            } else {
                format!(" {target}")
            };
            out.push_str(&format!("{name}:\t{ratio:6.1}% -- {verb}{suffix}\n"));
        }
        if to_stdout {
            out.push_str(&String::from_utf8_lossy(&result));
            continue;
        }
        let target_path = c.resolve(&target);
        if c.vfs.exists(&target_path) && !force {
            return Err(Fail::new(
                format!("{cmd}: {target} already exists; use -f to overwrite"),
                1,
            ));
        }
        c.vfs
            .write_as(&target_path, &result, &user, t)
            .map_err(|e| Fail::new(format!("{cmd}: {target}: {e}"), 1))?;
        let _ = c.vfs.chmod_as(&target_path, meta.mode, &user);
        let _ = c.vfs.set_times_as(
            &target_path,
            Some(meta.accessed),
            Some(meta.modified),
            &user,
            t,
            true,
        );
        if !keep {
            c.vfs
                .remove_as(&path, false, &user)
                .map_err(|e| Fail::new(format!("{cmd}: {name}: {e}"), 1))?;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// zip / unzip
// ---------------------------------------------------------------------------

/// DOS date and time fields, from a world-clock tick.
fn dos_stamp(tick: u64) -> (u16, u16) {
    let k = crate::shell::clock(tick);
    let time = ((k.hour as u16) << 11) | ((k.minute as u16) << 5) | (k.second as u16 / 2);
    let year = k.year.saturating_sub(1980).min(127) as u16;
    let date = (year << 9) | ((k.month as u16) << 5) | (k.day as u16);
    (time, date)
}
/// The inverse, so `unzip -l` prints the date the archive actually carries.
fn tick_from_dos(time: u16, date: u16) -> u64 {
    let (year, month, day) = (
        i64::from(1980 + (date >> 9)),
        i64::from((date >> 5) & 0xf).max(1),
        i64::from(date & 0x1f).max(1),
    );
    let (hour, minute, second) = (
        u64::from(time >> 11),
        u64::from((time >> 5) & 0x3f),
        u64::from((time & 0x1f) * 2),
    );
    let days = days_from_civil(year, month, day).max(0) as u64;
    tick_of(days * 86_400 + hour * 3600 + minute * 60 + second)
}

fn le16(b: &[u8], p: usize) -> Result<u16, String> {
    let s = b
        .get(p..p + 2)
        .ok_or_else(|| "truncated zip record".to_string())?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn le32(b: &[u8], p: usize) -> Result<u32, String> {
    let s = b
        .get(p..p + 4)
        .ok_or_else(|| "truncated zip record".to_string())?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

struct ZipEntry {
    name: String,
    mode: u16,
    tick: u64,
    data: Vec<u8>,
    is_dir: bool,
    /// True when the member arrived as method 8 and had to be inflated.
    deflated: bool,
}

fn read_zip(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    if bytes.len() < 22 {
        return Err("cannot find the end-of-central-directory record".into());
    }
    let eocd = (0..=bytes.len() - 22)
        .rev()
        .find(|&i| bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| "cannot find the end-of-central-directory record".to_string())?;
    let count = usize::from(le16(bytes, eocd + 10)?);
    let mut p = le32(bytes, eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(p..p + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err("central directory record is missing its signature".into());
        }
        let method = le16(bytes, p + 10)?;
        let time = le16(bytes, p + 12)?;
        let date = le16(bytes, p + 14)?;
        let crc = le32(bytes, p + 16)?;
        let csize = le32(bytes, p + 20)? as usize;
        let usize_ = le32(bytes, p + 24)? as usize;
        let name_len = usize::from(le16(bytes, p + 28)?);
        let extra_len = usize::from(le16(bytes, p + 30)?);
        let comment_len = usize::from(le16(bytes, p + 32)?);
        let external = le32(bytes, p + 38)?;
        let local = le32(bytes, p + 42)? as usize;
        let name = String::from_utf8_lossy(
            bytes
                .get(p + 46..p + 46 + name_len)
                .ok_or_else(|| "truncated central directory name".to_string())?,
        )
        .into_owned();
        p += 46 + name_len + extra_len + comment_len;
        if bytes.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(format!("{name}: local header is missing its signature"));
        }
        let local_name = usize::from(le16(bytes, local + 26)?);
        let local_extra = usize::from(le16(bytes, local + 28)?);
        let start = local + 30 + local_name + local_extra;
        let raw = bytes
            .get(start..start + csize)
            .ok_or_else(|| format!("{name}: compressed data runs past the end of the archive"))?;
        let is_dir = name.ends_with('/');
        let data = match method {
            0 => raw.to_vec(),
            8 => inflate(raw).map_err(|e| format!("{name}: {e}"))?.0,
            other => return Err(format!("{name}: unsupported compression method {other}")),
        };
        if !is_dir && (crc32(&data) != crc || data.len() != usize_) {
            return Err(format!("{name}: bad CRC or length"));
        }
        let mode = ((external >> 16) & 0o7777) as u16;
        out.push(ZipEntry {
            name,
            mode: if mode == 0 {
                if is_dir {
                    0o755
                } else {
                    0o644
                }
            } else {
                mode
            },
            tick: tick_from_dos(time, date),
            data,
            is_dir,
            deflated: method == 8,
        });
    }
    Ok(out)
}

fn write_zip(entries: &[ZipEntry]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let (time, date) = dos_stamp(e.tick);
        let crc = crc32(&e.data);
        let size = e.data.len() as u32;
        let offset = body.len() as u32;
        let name = e.name.as_bytes();
        body.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        body.extend_from_slice(&20u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes()); // method 0: stored
        body.extend_from_slice(&time.to_le_bytes());
        body.extend_from_slice(&date.to_le_bytes());
        body.extend_from_slice(&crc.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(name);
        body.extend_from_slice(&e.data);

        let external = (u32::from(e.mode) << 16) | u32::from(e.is_dir) << 4;
        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        central.extend_from_slice(&0x031eu16.to_le_bytes()); // made by Unix, zip 3.0
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&external.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let (cd_offset, cd_size) = (body.len() as u32, central.len() as u32);
    let count = entries.len() as u16;
    body.extend_from_slice(&central);
    body.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&cd_size.to_le_bytes());
    body.extend_from_slice(&cd_offset.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body
}

pub(crate) fn zip(c: &mut Computer, cmd: &str, args: &[String], t: u64) -> Result<String, Fail> {
    if cmd == "zip" {
        zip_create(c, args, t)
    } else {
        unzip(c, args, t)
    }
}

fn zip_create(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "zip",
        args,
        "rq",
        "",
        &[("recurse-paths", 'r'), ("quiet", 'q')],
    )?;
    let (recurse, quiet) = (flag(&opts, 'r'), flag(&opts, 'q'));
    let Some((name, files)) = operands.split_first() else {
        return Err(Fail::usage(
            "zip: ARCHIVE and at least one FILE are required",
        ));
    };
    if files.is_empty() {
        return Err(Fail::usage(
            "zip: ARCHIVE and at least one FILE are required",
        ));
    }
    let archive_name = if name.ends_with(".zip") {
        name.clone()
    } else {
        format!("{name}.zip")
    };
    let archive = c.resolve(&archive_name);
    let user = c.user.clone();
    let mut entries: Vec<ZipEntry> = Vec::new();
    let mut out = String::new();
    for operand in files {
        let stored_root = member_name(operand);
        if stored_root.is_empty() {
            return Err(Fail::usage(format!(
                "zip: {operand}: not a valid member name"
            )));
        }
        let root = c.resolve(operand);
        let meta = c
            .vfs
            .lstat(&root)
            .map_err(|_| Fail::new(format!("zip: {operand}: No such file or directory"), 1))?;
        let paths: Vec<String> = if meta.is_dir && recurse {
            walk_tree(c, &root).into_iter().map(|(p, _, _)| p).collect()
        } else {
            vec![root.clone()]
        };
        for path in paths {
            let suffix = path[root.len().min(path.len())..].trim_start_matches('/');
            let stored = if suffix.is_empty() {
                stored_root.clone()
            } else {
                format!("{stored_root}/{suffix}")
            };
            let meta = c.vfs.lstat(&path)?;
            // A symbolic link is stored as its target text, which is what `zip`
            // without `-y` does after following the link fails.
            let (data, is_dir) = if meta.is_dir {
                (Vec::new(), true)
            } else if meta.is_symlink {
                (c.vfs.read_link(&path)?.into_bytes(), false)
            } else {
                (
                    c.vfs
                        .read_as(&path, &user)
                        .map_err(|e| Fail::new(format!("zip: {stored}: {e}"), 1))?,
                    false,
                )
            };
            let stored = if is_dir { format!("{stored}/") } else { stored };
            if !quiet {
                out.push_str(&format!("  adding: {stored} (stored 0%)\n"));
            }
            entries.push(ZipEntry {
                name: stored,
                mode: meta.mode & 0o7777,
                tick: meta.modified,
                data,
                is_dir,
                deflated: false,
            });
        }
    }
    let bytes = write_zip(&entries);
    c.vfs
        .write_as(&archive, &bytes, &user, t)
        .map_err(|e| Fail::new(format!("zip: {archive_name}: {e}"), 1))?;
    Ok(out)
}

fn unzip(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "unzip",
        args,
        "loq",
        "d",
        &[
            ("list", 'l'),
            ("overwrite", 'o'),
            ("quiet", 'q'),
            ("directory", 'd'),
        ],
    )?;
    let Some((name, wanted)) = operands.split_first() else {
        return Err(Fail::usage("unzip: an ARCHIVE is required"));
    };
    let archive = c.resolve(name);
    let user = c.user.clone();
    let bytes = c.vfs.read_as(&archive, &user).map_err(|_| {
        Fail::new(
            format!("unzip: cannot find or open {name}, {name}.zip or {name}.ZIP."),
            1,
        )
    })?;
    let entries = read_zip(&bytes).map_err(|e| Fail::new(format!("unzip: {name}: {e}"), 1))?;
    let selected: Vec<&ZipEntry> = entries
        .iter()
        .filter(|e| wanted.is_empty() || wanted.contains(&e.name))
        .collect();
    let mut out = format!("Archive:  {archive}\n");
    if flag(&opts, 'l') {
        out.push_str("  Length      Date    Time    Name\n");
        out.push_str("---------  ---------- -----   ----\n");
        let mut total = 0u64;
        for e in &selected {
            total += e.data.len() as u64;
            out.push_str(&format!(
                "{:>9}  {}   {}\n",
                e.data.len(),
                stamp(e.tick),
                e.name
            ));
        }
        out.push_str("---------                     -------\n");
        out.push_str(&format!(
            "{:>9}                     {} file{}\n",
            total,
            selected.len(),
            if selected.len() == 1 { "" } else { "s" }
        ));
        return Ok(out);
    }
    let base = match value(&opts, 'd') {
        Some(dir) => {
            let dir = c.resolve(dir);
            c.vfs
                .mkdir_all_as(&dir, &user, t)
                .map_err(|e| Fail::new(format!("unzip: {dir}: {e}"), 1))?;
            dir
        }
        None => c.cwd.clone(),
    };
    let overwrite = flag(&opts, 'o');
    let quiet = flag(&opts, 'q');
    if quiet {
        out.clear();
    }
    for e in &selected {
        if e.name.starts_with('/') || e.name.split('/').any(|p| p == "..") {
            return Err(Fail::new(
                format!(
                    "unzip: {}: refusing a member that escapes the destination",
                    e.name
                ),
                1,
            ));
        }
        let path = crate::vfs::normalize_path(&base, &e.name);
        if e.is_dir {
            if !quiet {
                out.push_str(&format!("   creating: {}\n", e.name));
            }
            c.vfs
                .mkdir_all_as(&path, &user, e.tick)
                .map_err(|err| Fail::new(format!("unzip: {}: {err}", e.name), 1))?;
            let _ = c.vfs.chmod_as(&path, e.mode, &user);
            continue;
        }
        if c.vfs.exists(&path) && !overwrite {
            return Err(Fail::new(
                format!("unzip: {} already exists; use -o to overwrite", e.name),
                1,
            ));
        }
        if let Some((parent, _)) = path.rsplit_once('/') {
            c.vfs
                .mkdir_all_as(if parent.is_empty() { "/" } else { parent }, &user, t)
                .map_err(|err| Fail::new(format!("unzip: {}: {err}", e.name), 1))?;
        }
        if !quiet {
            let verb = if e.deflated {
                " inflating"
            } else {
                "extracting"
            };
            out.push_str(&format!("  {verb}: {}\n", e.name));
        }
        c.vfs
            .write_as(&path, &e.data, &user, e.tick)
            .map_err(|err| Fail::new(format!("unzip: {}: {err}", e.name), 1))?;
        let _ = c.vfs.chmod_as(&path, e.mode, &user);
        let _ = c
            .vfs
            .set_times_as(&path, Some(e.tick), Some(e.tick), &user, t, true);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// rsync
// ---------------------------------------------------------------------------

/// `--delete` has no short spelling; the sentinel keeps it out of the letter
/// namespace while still flowing through the shared option parser.
const DELETE: char = '\u{2}';

/// A `host:path`, `user@host:path` or `rsync://` operand. A single-letter prefix is a
/// Windows drive root, not a host.
fn is_remote(spec: &str) -> bool {
    if spec.starts_with("rsync://") {
        return true;
    }
    match spec.split_once(':') {
        Some((host, _)) => !host.is_empty() && !host.contains('/') && host.len() > 1,
        None => false,
    }
}

/// `rsync [-a] [-v] [-n|--dry-run] [--delete] SRC DST`, local trees only.
pub(crate) fn rsync(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, operands) = options(
        "rsync",
        args,
        "avn",
        "",
        &[
            ("archive", 'a'),
            ("verbose", 'v'),
            ("dry-run", 'n'),
            ("delete", DELETE),
        ],
    )?;
    let (archive, verbose) = (flag(&opts, 'a'), flag(&opts, 'v'));
    let (dry, delete) = (flag(&opts, 'n'), flag(&opts, DELETE));
    if operands.len() != 2 {
        return Err(Fail::usage(
            "rsync: exactly one SRC and one DST are required",
        ));
    }
    for spec in &operands {
        if is_remote(spec) {
            return Err(Fail::usage(format!(
                "rsync: {spec}: remote transfers are not simulated; this world has no rsync daemon or ssh transport"
            )));
        }
    }
    let user = c.user.clone();
    let (src_spec, dst_spec) = (&operands[0], &operands[1]);
    let src = c.resolve(src_spec);
    let src_meta = c.vfs.lstat(&src).map_err(|_| {
        Fail::new(
            format!("rsync: link_stat \"{src}\" failed: No such file or directory (2)"),
            1,
        )
    })?;
    let mut out = String::new();
    if verbose {
        out.push_str("sending incremental file list\n");
    }
    let (mut sent, mut listed, mut total) = (0u64, Vec::new(), 0u64);

    if !src_meta.is_dir {
        // A single file: the destination may be a directory to drop it into.
        let dst_dir = c.resolve(dst_spec);
        let target =
            if c.vfs.stat(&dst_dir).map(|m| m.is_dir).unwrap_or(false) || dst_spec.ends_with('/') {
                let name = src.rsplit_once('/').map(|(_, n)| n).unwrap_or(&src);
                format!("{}/{name}", dst_dir.trim_end_matches('/'))
            } else {
                dst_dir
            };
        let bytes = c
            .vfs
            .read_as(&src, &user)
            .map_err(|e| Fail::new(format!("rsync: {src}: {e}"), 1))?;
        total += bytes.len() as u64;
        if c.vfs.read(&target).ok().as_deref() != Some(bytes.as_slice()) {
            listed.push(
                src.rsplit_once('/')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| src.clone()),
            );
            sent += bytes.len() as u64;
            if !dry {
                copy_file(c, &src, &target, &user, archive, t)?;
            }
        }
        return Ok(finish(out, listed, sent, total, verbose, dry));
    }
    if !archive {
        // Without -a (and this shell has no separate -r) rsync will not recurse.
        out.push_str(&format!("skipping directory {src_spec}\n"));
        return Ok(finish(out, listed, sent, total, verbose, dry));
    }
    // The trailing-slash rule: `src/` copies the contents, `src` copies the directory.
    let dst_root = if src_spec.ends_with('/') {
        c.resolve(dst_spec)
    } else {
        let name = src.rsplit_once('/').map(|(_, n)| n).unwrap_or(&src);
        crate::vfs::normalize_path(&c.resolve(dst_spec), name)
    };
    if !dry {
        c.vfs
            .mkdir_all_as(&dst_root, &user, t)
            .map_err(|e| Fail::new(format!("rsync: {dst_root}: {e}"), 1))?;
    }
    let mut wanted = std::collections::BTreeSet::new();
    for (path, _, _) in walk_tree(c, &src) {
        let relative = path[src.len().min(path.len())..]
            .trim_start_matches('/')
            .to_string();
        wanted.insert(relative.clone());
        let target = if relative.is_empty() {
            dst_root.clone()
        } else {
            format!("{}/{relative}", dst_root.trim_end_matches('/'))
        };
        let meta = c.vfs.lstat(&path)?;
        if meta.is_dir {
            let existed = c.vfs.stat(&target).map(|m| m.is_dir).unwrap_or(false);
            if !relative.is_empty() && !existed {
                listed.push(format!("{relative}/"));
            }
            if !dry {
                c.vfs
                    .mkdir_all_as(&target, &user, t)
                    .map_err(|e| Fail::new(format!("rsync: {target}: {e}"), 1))?;
                if archive {
                    let _ = c.vfs.copy_attributes_as(&path, &target, &user);
                }
            }
            continue;
        }
        if meta.is_symlink {
            let link = c.vfs.read_link(&path)?;
            if c.vfs.read_link(&target).ok().as_deref() != Some(link.as_str()) {
                listed.push(format!("{relative} -> {link}"));
                if !dry {
                    if c.vfs.lstat(&target).is_ok() {
                        c.vfs.remove_as(&target, false, &user)?;
                    }
                    c.vfs.symlink_as(&link, &target, &user, meta.modified)?;
                }
            }
            continue;
        }
        let bytes = c
            .vfs
            .read_as(&path, &user)
            .map_err(|e| Fail::new(format!("rsync: {path}: {e}"), 1))?;
        total += bytes.len() as u64;
        // A quick check, as rsync's own default is: size and time, then content.
        let unchanged = c
            .vfs
            .lstat(&target)
            .ok()
            .filter(|m| !m.is_dir && m.size == bytes.len())
            .map(|_| c.vfs.read(&target).ok().as_deref() == Some(bytes.as_slice()))
            .unwrap_or(false);
        if unchanged {
            continue;
        }
        listed.push(relative);
        sent += bytes.len() as u64;
        if !dry {
            copy_file(c, &path, &target, &user, archive, t)?;
        }
    }
    if delete {
        let mut doomed: Vec<String> = Vec::new();
        for (path, _, _) in walk_tree(c, &dst_root) {
            let relative = path[dst_root.len().min(path.len())..]
                .trim_start_matches('/')
                .to_string();
            if relative.is_empty() || wanted.contains(&relative) {
                continue;
            }
            doomed.push(relative);
        }
        // Deepest first, so a directory is empty by the time it goes.
        doomed.sort();
        for relative in doomed.iter().rev() {
            out.push_str(&format!("deleting {relative}\n"));
            if !dry {
                let path = format!("{}/{relative}", dst_root.trim_end_matches('/'));
                if c.vfs.lstat(&path).is_ok() {
                    c.vfs
                        .remove_as(&path, true, &user)
                        .map_err(|e| Fail::new(format!("rsync: {path}: {e}"), 1))?;
                }
            }
        }
    }
    Ok(finish(out, listed, sent, total, verbose, dry))
}

fn copy_file(
    c: &mut Computer,
    from: &str,
    to: &str,
    user: &str,
    archive: bool,
    t: u64,
) -> Result<(), Fail> {
    let bytes = c
        .vfs
        .read_as(from, user)
        .map_err(|e| Fail::new(format!("rsync: {from}: {e}"), 1))?;
    if let Some((parent, _)) = to.rsplit_once('/') {
        c.vfs
            .mkdir_all_as(if parent.is_empty() { "/" } else { parent }, user, t)
            .map_err(|e| Fail::new(format!("rsync: {to}: {e}"), 1))?;
    }
    c.vfs
        .write_as(to, &bytes, user, t)
        .map_err(|e| Fail::new(format!("rsync: {to}: {e}"), 1))?;
    if archive {
        let _ = c.vfs.copy_attributes_as(from, to, user);
    }
    Ok(())
}

/// rsync's trailing summary. The whole sync happens inside one world tick, so the
/// rate line reports the transfer as a single second's worth rather than consulting
/// any host timer.
fn finish(
    mut out: String,
    listed: Vec<String>,
    sent: u64,
    total: u64,
    verbose: bool,
    dry: bool,
) -> String {
    if verbose {
        for name in &listed {
            out.push_str(name);
            out.push('\n');
        }
        // What comes back is the receiver's file list: one line per name.
        let received: u64 = listed.iter().map(|n| n.len() as u64 + 1).sum();
        let moved = sent + received;
        let speedup = if moved == 0 {
            0.0
        } else {
            total as f64 / moved as f64
        };
        out.push('\n');
        out.push_str(&format!(
            "sent {sent} bytes  received {received} bytes  {:.2} bytes/sec\n",
            moved as f64
        ));
        out.push_str(&format!("total size is {total}  speedup is {speedup:.2}"));
        out.push_str(if dry { " (DRY RUN)\n" } else { "\n" });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shell, Computer, OfflineHost};

    fn run(c: &mut Computer, line: &str) -> (String, String, i32) {
        let r = shell::execute(c, line, 1_000_000, &mut OfflineHost);
        (r.stdout, r.stderr, r.exit_code)
    }
    fn ok(c: &mut Computer, line: &str) -> String {
        let (out, err, code) = run(c, line);
        assert_eq!(code, 0, "`{line}` failed: {err}");
        out
    }
    /// A small tree: two files, a nested directory, a non-default mode and a symlink.
    fn machine() -> Computer {
        let mut c = Computer::new("box", "user", "linux", true);
        ok(&mut c, "mkdir -p /home/user/proj/sub");
        ok(&mut c, "printf 'alpha\\n' > /home/user/proj/a.txt");
        ok(&mut c, "printf 'beta beta\\n' > /home/user/proj/sub/b.txt");
        ok(&mut c, "chmod 700 /home/user/proj/sub/b.txt");
        ok(&mut c, "ln -s a.txt /home/user/proj/link");
        c
    }

    #[test]
    fn tar_round_trip_preserves_bytes_modes_and_symlinks() {
        let mut c = machine();
        ok(&mut c, "tar -cf /tmp/proj.tar -C /home/user proj");
        ok(&mut c, "mkdir -p /tmp/out");
        let listing = ok(&mut c, "tar -xvf /tmp/proj.tar -C /tmp/out");
        assert!(listing.contains("proj/a.txt"), "{listing}");
        assert_eq!(ok(&mut c, "cat /tmp/out/proj/a.txt"), "alpha\n");
        assert_eq!(ok(&mut c, "cat /tmp/out/proj/sub/b.txt"), "beta beta\n");
        let mode = c.vfs.lstat("/tmp/out/proj/sub/b.txt").unwrap().mode;
        assert_eq!(mode & 0o777, 0o700, "extraction must restore the mode");
        let meta = c.vfs.lstat("/tmp/out/proj/link").unwrap();
        assert!(meta.is_symlink, "the symlink must come back as a symlink");
        assert_eq!(c.vfs.read_link("/tmp/out/proj/link").unwrap(), "a.txt");
        let original = c.vfs.lstat("/home/user/proj/a.txt").unwrap().modified;
        let restored = c.vfs.lstat("/tmp/out/proj/a.txt").unwrap().modified;
        assert_eq!(
            original / 1_000_000,
            restored / 1_000_000,
            "the mtime must survive to the second the header stores"
        );
    }

    #[test]
    fn tar_header_carries_ustar_magic_and_a_valid_checksum() {
        let mut c = machine();
        ok(&mut c, "tar -cf /tmp/proj.tar -C /home/user proj");
        let bytes = c.vfs.read("/tmp/proj.tar").unwrap();
        assert_eq!(&bytes[257..263], b"ustar\0", "ustar magic at offset 257");
        assert_eq!(&bytes[263..265], b"00", "ustar version at offset 263");
        assert_eq!(&bytes[..5], b"proj/", "the first member is the directory");
        assert_eq!(bytes[156], b'5', "typeflag 5 for a directory");
        let stored = parse_octal(&bytes[148..156]);
        let sum: u32 = bytes[..BLOCK]
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u32::from(*b)
                }
            })
            .sum();
        assert_eq!(u64::from(sum), stored, "the header checksum must be right");
        assert_eq!(
            bytes.len() % BLOCK,
            0,
            "the archive is a whole number of blocks"
        );
        let tail = &bytes[bytes.len() - 2 * BLOCK..];
        assert!(
            tail.iter().all(|b| *b == 0),
            "two zero blocks end the archive"
        );
    }

    #[test]
    fn tar_honours_directory_and_strip_components() {
        let mut c = machine();
        ok(&mut c, "tar -cf /tmp/proj.tar -C /home/user proj");
        ok(&mut c, "mkdir -p /tmp/flat");
        ok(
            &mut c,
            "tar -xf /tmp/proj.tar -C /tmp/flat --strip-components=1",
        );
        assert_eq!(ok(&mut c, "cat /tmp/flat/a.txt"), "alpha\n");
        assert_eq!(ok(&mut c, "cat /tmp/flat/sub/b.txt"), "beta beta\n");
        assert!(
            !c.vfs.exists("/tmp/flat/proj"),
            "the stripped component must not be recreated"
        );
    }

    #[test]
    fn tar_list_prints_names_and_a_long_listing() {
        let mut c = machine();
        ok(&mut c, "tar -cf /tmp/proj.tar -C /home/user proj");
        let plain = ok(&mut c, "tar -tf /tmp/proj.tar");
        let names: Vec<&str> = plain.lines().collect();
        assert_eq!(
            names,
            vec![
                "proj/",
                "proj/a.txt",
                "proj/link",
                "proj/sub/",
                "proj/sub/b.txt"
            ]
        );
        let long = ok(&mut c, "tar -tvf /tmp/proj.tar");
        assert!(long.contains("-rw"), "{long}");
        assert!(long.contains("user/user"), "{long}");
        assert!(long.contains("link -> a.txt"), "{long}");
        assert!(long.contains("2026-09-17"), "{long}");
    }

    #[test]
    fn tar_gzip_round_trip_writes_a_real_gzip_member() {
        let mut c = machine();
        ok(&mut c, "tar -czf /tmp/proj.tgz -C /home/user proj");
        let bytes = c.vfs.read("/tmp/proj.tgz").unwrap();
        assert_eq!(&bytes[..3], &[0x1f, 0x8b, 0x08], "gzip magic and CM=8");
        assert_eq!(bytes[9], 3, "OS byte is Unix");
        ok(&mut c, "mkdir -p /tmp/gz");
        ok(&mut c, "tar -xzf /tmp/proj.tgz -C /tmp/gz");
        assert_eq!(ok(&mut c, "cat /tmp/gz/proj/sub/b.txt"), "beta beta\n");
        let names = ok(&mut c, "tar -tzf /tmp/proj.tgz");
        assert!(names.contains("proj/a.txt"), "{names}");
    }

    #[test]
    fn gzip_gunzip_and_zcat_round_trip_with_keep_and_stdout() {
        let mut c = machine();
        ok(&mut c, "gzip /home/user/proj/a.txt");
        assert!(
            !c.vfs.exists("/home/user/proj/a.txt"),
            "the source is replaced"
        );
        let gz = c.vfs.read("/home/user/proj/a.txt.gz").unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b], "RFC 1952 magic");
        assert_eq!(
            u32::from_le_bytes([
                gz[gz.len() - 4],
                gz[gz.len() - 3],
                gz[gz.len() - 2],
                gz[gz.len() - 1]
            ]),
            6,
            "ISIZE is the uncompressed length"
        );
        assert_eq!(ok(&mut c, "zcat /home/user/proj/a.txt.gz"), "alpha\n");
        assert!(
            c.vfs.exists("/home/user/proj/a.txt.gz"),
            "zcat must leave the archive alone"
        );
        ok(&mut c, "gunzip -k /home/user/proj/a.txt.gz");
        assert_eq!(ok(&mut c, "cat /home/user/proj/a.txt"), "alpha\n");
        assert!(
            c.vfs.exists("/home/user/proj/a.txt.gz"),
            "-k keeps the source"
        );
        // -c leaves both sides untouched and writes to stdout.
        assert_eq!(ok(&mut c, "gunzip -c /home/user/proj/a.txt.gz"), "alpha\n");
        assert!(c.vfs.exists("/home/user/proj/a.txt.gz"));
    }

    #[test]
    fn gzip_refuses_a_double_suffix_and_an_unreadable_member() {
        let mut c = machine();
        ok(&mut c, "gzip -k /home/user/proj/a.txt");
        let (_, err, code) = run(&mut c, "gzip /home/user/proj/a.txt.gz");
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("already has .gz suffix"), "{err}");
        let (_, err, code) = run(&mut c, "gzip /home/user/proj/a.txt");
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("already exists"), "{err}");
        c.vfs
            .write(
                "/tmp/bogus.gz",
                b"not a gzip file at all............",
                "user",
                0,
            )
            .unwrap();
        let (_, err, code) = run(&mut c, "gunzip /tmp/bogus.gz");
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("not in gzip format"), "{err}");
    }

    /// A fixed-Huffman and a dynamic-Huffman DEFLATE stream produced by zlib, so the
    /// inflate paths our own stored-block writer never reaches are really exercised.
    #[test]
    fn inflate_reads_fixed_and_dynamic_huffman_streams() {
        const FIXED: [u8; 18] = [
            0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x22, 0xcb, 0xf3, 0x8b, 0x72, 0x52,
            0x90, 0x49, 0x2e, 0x00,
        ];
        assert_eq!((FIXED[0] >> 1) & 3, 1, "the fixture is a BTYPE=01 block");
        let (out, _) = inflate(&FIXED).expect("fixed-Huffman stream");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "hello hello hello world world world\n"
        );
        const DYNAMIC: [u8; 45] = [
            0x8d, 0xcb, 0xb1, 0x01, 0x00, 0x20, 0x08, 0x03, 0xc1, 0x59, 0x21, 0x80, 0x84, 0xb8,
            0x7f, 0xad, 0x03, 0x58, 0xd8, 0x7d, 0x71, 0xdf, 0x43, 0x8b, 0x94, 0x14, 0xa8, 0xdc,
            0x6d, 0xdc, 0x30, 0xc5, 0xdc, 0xc2, 0x94, 0x67, 0x81, 0xa2, 0x2f, 0xab, 0x7e, 0xb8,
            0xfe, 0x7c, 0x0f,
        ];
        assert_eq!((DYNAMIC[0] >> 1) & 3, 2, "the fixture is a BTYPE=10 block");
        let (out, _) = inflate(&DYNAMIC).expect("dynamic-Huffman stream");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "hjiadekkkdcfelhailcakdjlhacjfbefcikibgaf\
             hjiadekkkdcfelhailcahjiadekkkdcfelhailca\
             kdjlhacjfbefcikibgaf",
            "the back-references must be resolved, not skipped"
        );
    }

    /// A whole gzip member written by a host tool, FNAME flag and dynamic Huffman
    /// included, read back through the shell.
    #[test]
    fn gunzip_reads_a_host_written_member() {
        const HOST: [u8; 103] = [
            0x1f, 0x8b, 0x08, 0x08, 0x10, 0xac, 0xab, 0x6a, 0x02, 0x03, 0x70, 0x61, 0x79, 0x6c,
            0x6f, 0x61, 0x64, 0x2e, 0x62, 0x69, 0x6e, 0x00, 0xbd, 0x8e, 0xb1, 0x01, 0x80, 0x00,
            0x08, 0xc3, 0x6e, 0x05, 0x0a, 0x05, 0xca, 0xff, 0xb3, 0xbe, 0xe0, 0xe2, 0x9a, 0x0c,
            0x89, 0x5b, 0xda, 0x9c, 0xe9, 0x60, 0x83, 0xa3, 0x98, 0x16, 0x58, 0x88, 0xd7, 0xaf,
            0x18, 0x12, 0x6d, 0xd7, 0x9a, 0x88, 0x78, 0xd9, 0xba, 0x46, 0x7d, 0x28, 0x4c, 0xdc,
            0xd0, 0xb3, 0x2f, 0x00, 0x4e, 0x57, 0xaf, 0x7a, 0x6b, 0xb9, 0x4a, 0xaa, 0x54, 0x49,
            0xd4, 0x76, 0xc5, 0x04, 0xdd, 0x3f, 0x37, 0xfc, 0x87, 0xab, 0x07, 0x20, 0xa9, 0xb6,
            0x0c, 0xfa, 0x00, 0x00, 0x00,
        ];
        let mut c = machine();
        c.vfs.write("/tmp/host.gz", &HOST, "user", 0).unwrap();
        let out = ok(&mut c, "zcat /tmp/host.gz");
        assert_eq!(out.len(), 250);
        assert!(out.starts_with("baeailakldaidlgkgeacdjdkglheaiiggdhalhkiccckgljbki"));
        // A single flipped byte inside the payload must be caught by the CRC.
        let mut corrupt = HOST;
        corrupt[40] ^= 0x01;
        c.vfs.write("/tmp/bad.gz", &corrupt, "user", 0).unwrap();
        let (_, err, code) = run(&mut c, "zcat /tmp/bad.gz");
        assert_eq!(code, 1, "a corrupt member must not succeed");
        assert!(!err.is_empty(), "{err}");
    }

    #[test]
    fn zip_and_unzip_round_trip_with_a_listing() {
        let mut c = machine();
        ok(&mut c, "cd /home/user && zip -r /tmp/proj.zip proj");
        let bytes = c.vfs.read("/tmp/proj.zip").unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04", "local file header signature");
        assert!(
            bytes.windows(4).any(|w| w == b"PK\x01\x02"),
            "a central directory record must be present"
        );
        assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
        let listing = ok(&mut c, "unzip -l /tmp/proj.zip");
        assert!(
            listing.starts_with("Archive:  /tmp/proj.zip\n"),
            "{listing}"
        );
        assert!(
            listing.contains(
                "  Length      Date    Time    Name\n---------  ---------- -----   ----\n"
            ),
            "{listing}"
        );
        assert!(listing.contains("proj/a.txt"), "{listing}");
        assert!(listing.contains("5 files"), "{listing}");
        ok(&mut c, "mkdir -p /tmp/unz");
        let extracted = ok(&mut c, "unzip -o -d /tmp/unz /tmp/proj.zip");
        assert!(extracted.contains("extracting: proj/a.txt"), "{extracted}");
        assert_eq!(ok(&mut c, "cat /tmp/unz/proj/a.txt"), "alpha\n");
        assert_eq!(ok(&mut c, "cat /tmp/unz/proj/sub/b.txt"), "beta beta\n");
        assert_eq!(
            c.vfs.lstat("/tmp/unz/proj/sub/b.txt").unwrap().mode & 0o777,
            0o700
        );
        // Without -o an existing member is refused rather than silently clobbered.
        let (_, err, code) = run(&mut c, "unzip -d /tmp/unz /tmp/proj.zip");
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("use -o to overwrite"), "{err}");
        // Naming members extracts only those, and -q suppresses the chatter.
        ok(&mut c, "mkdir -p /tmp/one");
        let quiet = ok(&mut c, "unzip -q -d /tmp/one /tmp/proj.zip proj/a.txt");
        assert_eq!(quiet, "", "-q prints nothing");
        assert!(c.vfs.exists("/tmp/one/proj/a.txt"));
        assert!(
            !c.vfs.exists("/tmp/one/proj/sub/b.txt"),
            "an unnamed member must stay in the archive"
        );
    }

    #[test]
    fn unzip_inflates_a_deflated_member() {
        // A method-8 member: the same dynamic-Huffman payload, wrapped by hand.
        const DEFLATED: [u8; 45] = [
            0x8d, 0xcb, 0xb1, 0x01, 0x00, 0x20, 0x08, 0x03, 0xc1, 0x59, 0x21, 0x80, 0x84, 0xb8,
            0x7f, 0xad, 0x03, 0x58, 0xd8, 0x7d, 0x71, 0xdf, 0x43, 0x8b, 0x94, 0x14, 0xa8, 0xdc,
            0x6d, 0xdc, 0x30, 0xc5, 0xdc, 0xc2, 0x94, 0x67, 0x81, 0xa2, 0x2f, 0xab, 0x7e, 0xb8,
            0xfe, 0x7c, 0x0f,
        ];
        let plain = b"hjiadekkkdcfelhailcakdjlhacjfbefcikibgafhjiadekkkdcfelhailcahjiadekkkdcfelhailcakdjlhacjfbefcikibgaf";
        let name = b"payload.txt";
        let (crc, size) = (crc32(plain), plain.len() as u32);
        let mut local = Vec::new();
        local.extend_from_slice(b"PK\x03\x04");
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&8u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0x5931u16.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(DEFLATED.len() as u32).to_le_bytes());
        local.extend_from_slice(&size.to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name);
        local.extend_from_slice(&DEFLATED);
        let offset = 0u32;
        let mut central = Vec::new();
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&0x031eu16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x5931u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(DEFLATED.len() as u32).to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 8]);
        central.extend_from_slice(&(0o644u32 << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
        let (cd_offset, cd_size) = (local.len() as u32, central.len() as u32);
        let mut archive = local;
        archive.extend_from_slice(&central);
        archive.extend_from_slice(b"PK\x05\x06");
        archive.extend_from_slice(&[0u8; 4]);
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&cd_size.to_le_bytes());
        archive.extend_from_slice(&cd_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());

        let mut c = machine();
        c.vfs
            .write("/tmp/deflated.zip", &archive, "user", 0)
            .unwrap();
        ok(&mut c, "mkdir -p /tmp/defl");
        let out = ok(&mut c, "unzip -d /tmp/defl /tmp/deflated.zip");
        assert!(out.contains("inflating: payload.txt"), "{out}");
        assert_eq!(
            c.vfs.read("/tmp/defl/payload.txt").unwrap(),
            plain.to_vec(),
            "a method-8 member must be inflated, not copied"
        );
    }

    #[test]
    fn rsync_copies_a_tree_deletes_and_dry_runs() {
        let mut c = machine();
        let out = ok(&mut c, "rsync -av /home/user/proj/ /tmp/mirror");
        assert!(out.starts_with("sending incremental file list\n"), "{out}");
        assert!(out.contains("a.txt"), "{out}");
        assert!(out.contains("sub/"), "{out}");
        assert!(out.contains("total size is "), "{out}");
        assert_eq!(ok(&mut c, "cat /tmp/mirror/a.txt"), "alpha\n");
        assert_eq!(ok(&mut c, "cat /tmp/mirror/sub/b.txt"), "beta beta\n");
        assert_eq!(
            c.vfs.lstat("/tmp/mirror/sub/b.txt").unwrap().mode & 0o777,
            0o700,
            "-a preserves the mode"
        );
        // Without a trailing slash the directory itself is copied.
        ok(&mut c, "rsync -a /home/user/proj /tmp/holder");
        assert_eq!(ok(&mut c, "cat /tmp/holder/proj/a.txt"), "alpha\n");
        // A second run transfers nothing.
        let again = ok(&mut c, "rsync -av /home/user/proj/ /tmp/mirror");
        assert!(again.contains("sent 0 bytes"), "{again}");
        // --delete removes what the source no longer has.
        ok(&mut c, "printf 'x\\n' > /tmp/mirror/stale.txt");
        let deleted = ok(&mut c, "rsync -av --delete /home/user/proj/ /tmp/mirror");
        assert!(deleted.contains("deleting stale.txt"), "{deleted}");
        assert!(!c.vfs.exists("/tmp/mirror/stale.txt"));
        // -n says what it would do and does none of it.
        ok(&mut c, "printf 'new\\n' > /home/user/proj/c.txt");
        let dry = ok(&mut c, "rsync -avn /home/user/proj/ /tmp/mirror");
        assert!(dry.contains("c.txt"), "{dry}");
        assert!(dry.contains("(DRY RUN)"), "{dry}");
        assert!(
            !c.vfs.exists("/tmp/mirror/c.txt"),
            "a dry run must change nothing"
        );
    }

    #[test]
    fn rsync_refuses_a_remote_spec_by_name() {
        let mut c = machine();
        for line in [
            "rsync -av /home/user/proj/ backup:/srv/proj",
            "rsync -av rsync://host/mod /tmp/x",
        ] {
            let (_, err, code) = run(&mut c, line);
            assert_eq!(code, 2, "a remote transfer must be refused: {err}");
            assert!(err.contains("remote transfers are not simulated"), "{err}");
        }
    }

    #[test]
    fn every_command_refuses_an_unknown_flag_by_name() {
        let mut c = machine();
        for (line, flag) in [
            ("tar -cQf /tmp/x.tar -C /home/user proj", "-Q"),
            ("gzip -Q /home/user/proj/a.txt", "-Q"),
            ("gunzip --bogus /tmp/x.gz", "--bogus"),
            ("zcat -Q /tmp/x.gz", "-Q"),
            ("zip -Q /tmp/x.zip /home/user/proj", "-Q"),
            ("unzip -Q /tmp/x.zip", "-Q"),
            ("rsync -Q /home/user/proj/ /tmp/x", "-Q"),
            (
                "rsync --bandwidth-limit=5 /home/user/proj/ /tmp/x",
                "--bandwidth-limit",
            ),
        ] {
            let (_, err, code) = run(&mut c, line);
            assert_eq!(code, 2, "`{line}` must exit 2, not {code}: {err}");
            assert!(err.contains(flag), "`{line}` must name {flag}: {err}");
        }
        // tar refuses a pipe archive by name rather than pretending.
        let (_, err, code) = run(&mut c, "tar -cf - /home/user/proj");
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("-f -"), "{err}");
        // ... and insists on exactly one mode.
        let (_, err, code) = run(&mut c, "tar -f /tmp/x.tar /home/user/proj");
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("-c, -x or -t"), "{err}");
    }
}
