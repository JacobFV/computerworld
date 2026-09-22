//! A resumable DEFLATE decoder (RFC 1950/1951/1952) with zlib's error messages.
//!
//! Input may arrive in pieces: decoding advances symbol by symbol and stops,
//! with its state intact, when the next symbol (or block header) is not wholly
//! available, so a stream decoder can feed chunks as they come.
use crate::{adler32, crc32, ZError};

const MAXBITS: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Raw,
    Zlib,
    Gzip,
    /// zlib or gzip, whichever the header says (`windowBits + 32`).
    Auto,
}

#[derive(Clone)]
struct Huffman {
    count: [u16; MAXBITS + 1],
    symbol: Vec<u16>,
    /// Fast table: 2^FAST entries of (symbol, length); length 0 = slow path.
    fast: Vec<(u16, u8)>,
}
const FAST: usize = 9;

impl Huffman {
    /// Builds a canonical decoder; `Err(())` when the lengths are
    /// over-subscribed or (except as zlib allows) incomplete.
    fn new(lengths: &[u8], codes: bool) -> Result<Self, ()> {
        let mut count = [0u16; MAXBITS + 1];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut max = MAXBITS;
        while max >= 1 && count[max] == 0 {
            max -= 1;
        }
        let mut left: i32 = 1;
        for c in count.iter().take(MAXBITS + 1).skip(1) {
            left <<= 1;
            left -= *c as i32;
            if left < 0 {
                return Err(());
            }
        }
        if max > 0 && left > 0 && (codes || max != 1) {
            return Err(());
        }
        let mut offs = [0u16; MAXBITS + 2];
        for len in 1..=MAXBITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        let mut h = Huffman {
            count,
            symbol,
            fast: vec![(0, 0); 1 << FAST],
        };
        // Fill the fast table by enumerating codes of length <= FAST.
        let mut code: u32 = 0;
        let mut index = 0usize;
        for len in 1..=MAXBITS {
            for _ in 0..h.count[len] {
                if len <= FAST {
                    let rev = reverse(code, len as u32);
                    let step = 1usize << len;
                    let mut i = rev as usize;
                    while i < (1 << FAST) {
                        h.fast[i] = (h.symbol[index], len as u8);
                        i += step;
                    }
                }
                code += 1;
                index += 1;
            }
            code <<= 1;
        }
        Ok(h)
    }
}

fn reverse(mut code: u32, len: u32) -> u32 {
    let mut r = 0;
    for _ in 0..len {
        r = (r << 1) | (code & 1);
        code >>= 1;
    }
    r
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Header,
    BlockHeader,
    Stored(usize),
    Codes,
    Check,
    Done,
}

#[derive(Clone, Copy)]
struct Bits {
    pos: usize,
    hold: u64,
    bits: u32,
}

/// Why decoding stopped without an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Needs more input.
    NeedInput,
    /// Reached `max_output` bytes.
    OutputFull,
    /// The stream ended (trailer verified); `unused` bytes of input follow it.
    End,
    /// A zlib header asked for a preset dictionary (Adler-32 of it given).
    NeedDict(u32),
}

#[derive(Clone)]
pub struct Inflater {
    wrap: Wrap,
    window_bits: u32,
    input: Vec<u8>,
    b: Bits,
    mode: Mode,
    last: bool,
    lit: Option<Huffman>,
    dist: Option<Huffman>,
    /// Everything decoded so far that is still inside the window (32 KiB).
    window: Vec<u8>,
    total_out: u64,
    check: u32,
    is_gzip: bool,
    dict_id: Option<u32>,
    dict_set: bool,
    /// Pending length/distance copy (resumes after an output limit).
    copy: Option<(usize, usize)>,
}

const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn data(msg: &str) -> ZError {
    ZError::Data(msg.to_string())
}

impl Inflater {
    /// `window_bits` as zlib takes it: 8..15 zlib, -8..-15 raw, 24..31 gzip,
    /// 40..47 automatic, 0 = use the window size in the header.
    pub fn new(window_bits: i32) -> Result<Self, ZError> {
        let (wrap, wb) = if window_bits < 0 {
            if window_bits < -15 {
                return Err(ZError::Stream);
            }
            (Wrap::Raw, (-window_bits) as u32)
        } else if window_bits == 0 {
            (Wrap::Zlib, 15)
        } else if window_bits >= 32 {
            (Wrap::Auto, (window_bits - 32).clamp(8, 15) as u32)
        } else if window_bits >= 16 {
            (Wrap::Gzip, (window_bits - 16) as u32)
        } else {
            (Wrap::Zlib, window_bits as u32)
        };
        if !(8..=15).contains(&wb) {
            return Err(ZError::Stream);
        }
        Ok(Self {
            wrap,
            window_bits: if window_bits == 0 { 15 } else { wb },
            input: vec![],
            b: Bits {
                pos: 0,
                hold: 0,
                bits: 0,
            },
            mode: if wrap == Wrap::Raw {
                Mode::BlockHeader
            } else {
                Mode::Header
            },
            last: false,
            lit: None,
            dist: None,
            window: vec![],
            total_out: 0,
            check: 0,
            is_gzip: false,
            dict_id: None,
            dict_set: false,
            copy: None,
        })
    }

    pub fn is_gzip(&self) -> bool {
        self.is_gzip
    }
    pub fn finished(&self) -> bool {
        self.mode == Mode::Done
    }
    pub fn total_out(&self) -> u64 {
        self.total_out
    }

    /// Input received but not yet decoded (after the end: data that follows the
    /// stream). Whole bytes held in the bit buffer are returned as unread.
    pub fn unconsumed(&self) -> &[u8] {
        let held = (self.b.bits / 8) as usize;
        &self.input[self.b.pos - held..]
    }

    /// Removes and returns the input not yet decoded (see [`Inflater::unconsumed`]),
    /// for callers that hand unread input back (`unconsumed_tail`, `unused_data`).
    pub fn take_unconsumed(&mut self) -> Vec<u8> {
        let held = self.b.bits / 8;
        let keep_bits = self.b.bits % 8;
        self.b.hold &= (1u64 << keep_bits) - 1;
        self.b.bits = keep_bits;
        self.b.pos -= held as usize;
        let rest = self.input.split_off(self.b.pos);
        self.input.drain(..self.b.pos);
        self.b.pos = 0;
        rest
    }

    /// `inflateSetDictionary`.
    pub fn set_dictionary(&mut self, dict: &[u8]) -> Result<(), ZError> {
        if let Some(id) = self.dict_id {
            if adler32(1, dict) != id {
                return Err(data("invalid dictionary"));
            }
        } else if self.wrap != Wrap::Raw {
            return Err(ZError::Stream);
        }
        let keep = 1usize << self.window_bits;
        let d = if dict.len() > keep {
            &dict[dict.len() - keep..]
        } else {
            dict
        };
        self.window.extend_from_slice(d);
        self.dict_set = true;
        if self.dict_id.is_some() {
            self.dict_id = None;
            self.mode = Mode::BlockHeader;
        }
        Ok(())
    }

    fn need(&mut self, n: u32) -> bool {
        while self.b.bits < n {
            if self.b.pos >= self.input.len() {
                return false;
            }
            self.b.hold |= (self.input[self.b.pos] as u64) << self.b.bits;
            self.b.pos += 1;
            self.b.bits += 8;
        }
        true
    }
    fn take(&mut self, n: u32) -> u32 {
        let v = (self.b.hold & ((1u64 << n) - 1)) as u32;
        self.b.hold >>= n;
        self.b.bits -= n;
        v
    }
    fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if self.need(n) {
            Some(self.take(n))
        } else {
            None
        }
    }
    fn align(&mut self) {
        let r = self.b.bits % 8;
        self.take(r);
    }
    fn byte(&mut self) -> Option<u8> {
        self.bits(8).map(|v| v as u8)
    }

    fn decode(&mut self, h: &Huffman) -> Result<Option<u16>, ZError> {
        // Fast path with whatever bits are available.
        let _ = self.need(FAST as u32);
        let avail = self.b.bits.min(FAST as u32);
        let idx = (self.b.hold & ((1u64 << FAST) - 1)) as usize;
        let (sym, len) = h.fast[idx];
        if len != 0 && (len as u32) <= avail {
            self.take(len as u32);
            return Ok(Some(sym));
        }
        // Slow path (puff's canonical decode), without consuming until complete.
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAXBITS {
            if !self.need(len as u32) {
                return Ok(None);
            }
            let bit = ((self.b.hold >> (len - 1)) & 1) as i32;
            code |= bit;
            let count = h.count[len] as i32;
            if code - count < first {
                self.take(len as u32);
                return Ok(Some(h.symbol[(index + (code - first)) as usize]));
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(ZError::Data(String::new()))
    }

    fn emit(&mut self, out: &mut Vec<u8>, byte: u8) {
        out.push(byte);
        self.window.push(byte);
    }

    fn trim_window(&mut self) {
        let keep = 1usize << 15;
        if self.window.len() > 4 * keep {
            let cut = self.window.len() - keep;
            self.window.drain(..cut);
        }
    }

    /// Decodes as much as possible from everything fed so far into `out`,
    /// stopping at `max_output` bytes of new output.
    pub fn inflate(
        &mut self,
        input: &[u8],
        out: &mut Vec<u8>,
        max_output: usize,
    ) -> Result<Progress, ZError> {
        // Keep only unread input (the bit buffer may hold bytes already read).
        if self.b.pos > 0 {
            self.input.drain(..self.b.pos);
            self.b.pos = 0;
        }
        self.input.extend_from_slice(input);
        let start = out.len();
        let r = self.run(out, start, max_output);
        let produced = &out[start..];
        match self.check_kind() {
            1 => self.check = adler32(self.check, produced),
            2 => self.check = crc32(self.check, produced),
            _ => {}
        }
        self.total_out += produced.len() as u64;
        self.trim_window();
        let r = r?;
        if r == Progress::End || self.mode == Mode::Check {
            // Verified below in Check mode.
        }
        Ok(r)
    }

    fn check_kind(&self) -> u8 {
        if self.is_gzip {
            2
        } else if matches!(self.wrap, Wrap::Zlib | Wrap::Auto) {
            1
        } else {
            0
        }
    }

    fn run(
        &mut self,
        out: &mut Vec<u8>,
        start: usize,
        max_output: usize,
    ) -> Result<Progress, ZError> {
        loop {
            if out.len() - start >= max_output && !matches!(self.mode, Mode::Check | Mode::Done) {
                return Ok(Progress::OutputFull);
            }
            match self.mode {
                Mode::Header => {
                    let save = self.b;
                    match self.header()? {
                        Some(()) => {}
                        None => {
                            self.b = save;
                            return Ok(Progress::NeedInput);
                        }
                    }
                    if let Some(id) = self.dict_id {
                        if !self.dict_set {
                            return Ok(Progress::NeedDict(id));
                        }
                    }
                }
                Mode::BlockHeader => {
                    if self.last {
                        self.mode = Mode::Check;
                        continue;
                    }
                    let save = self.b;
                    match self.block_header()? {
                        Some(()) => {}
                        None => {
                            self.b = save;
                            return Ok(Progress::NeedInput);
                        }
                    }
                }
                Mode::Stored(mut left) => {
                    while left > 0 {
                        if out.len() - start >= max_output {
                            self.mode = Mode::Stored(left);
                            return Ok(Progress::OutputFull);
                        }
                        // Stored bytes come from the byte stream (bit buffer empty).
                        match self.byte() {
                            Some(v) => {
                                self.emit(out, v);
                                left -= 1;
                            }
                            None => {
                                self.mode = Mode::Stored(left);
                                return Ok(Progress::NeedInput);
                            }
                        }
                    }
                    self.mode = Mode::BlockHeader;
                }
                Mode::Codes => {
                    if let Some(p) = self.codes(out, start, max_output)? {
                        return Ok(p);
                    }
                    self.mode = Mode::BlockHeader;
                }
                Mode::Check => {
                    // Checksums are computed over output; bring them up to date.
                    let produced = out[start..].to_vec();
                    let kind = self.check_kind();
                    let current = match kind {
                        1 => adler32(self.check, &produced),
                        2 => crc32(self.check, &produced),
                        _ => 0,
                    };
                    let total = self.total_out + produced.len() as u64;
                    let save = self.b;
                    self.align();
                    match kind {
                        1 => {
                            let mut v = 0u32;
                            for _ in 0..4 {
                                match self.byte() {
                                    Some(b) => v = (v << 8) | b as u32,
                                    None => {
                                        self.b = save;
                                        return Ok(Progress::NeedInput);
                                    }
                                }
                            }
                            if v != current {
                                self.b = save;
                                return Err(data("incorrect data check"));
                            }
                        }
                        2 => {
                            let mut v = [0u8; 8];
                            for x in v.iter_mut() {
                                match self.byte() {
                                    Some(b) => *x = b,
                                    None => {
                                        self.b = save;
                                        return Ok(Progress::NeedInput);
                                    }
                                }
                            }
                            let crc = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
                            let isize = u32::from_le_bytes([v[4], v[5], v[6], v[7]]);
                            if crc != current {
                                self.b = save;
                                return Err(data("incorrect data check"));
                            }
                            if isize != total as u32 {
                                self.b = save;
                                return Err(data("incorrect length check"));
                            }
                        }
                        _ => {}
                    }
                    self.mode = Mode::Done;
                }
                Mode::Done => return Ok(Progress::End),
            }
        }
    }

    fn header(&mut self) -> Result<Option<()>, ZError> {
        let gzip = match self.wrap {
            Wrap::Gzip => true,
            Wrap::Zlib => false,
            Wrap::Auto => {
                if !self.need(16) {
                    return Ok(None);
                }
                (self.b.hold & 0xffff) == 0x8b1f
            }
            Wrap::Raw => unreachable!(),
        };
        if gzip {
            self.is_gzip = true;
            let (Some(id1), Some(id2)) = (self.byte(), self.byte()) else {
                return Ok(None);
            };
            if id1 != 0x1f || id2 != 0x8b {
                return Err(data("incorrect header check"));
            }
            let Some(cm) = self.byte() else {
                return Ok(None);
            };
            if cm != 8 {
                return Err(data("unknown compression method"));
            }
            let Some(flags) = self.byte() else {
                return Ok(None);
            };
            if flags & 0xe0 != 0 {
                return Err(data("unknown header flags set"));
            }
            let mut hdr = vec![0x1f, 0x8b, cm, flags];
            for _ in 0..6 {
                let Some(b) = self.byte() else {
                    return Ok(None);
                };
                hdr.push(b);
            }
            if flags & 4 != 0 {
                let (Some(a), Some(b)) = (self.byte(), self.byte()) else {
                    return Ok(None);
                };
                hdr.extend([a, b]);
                let len = u16::from_le_bytes([a, b]) as usize;
                for _ in 0..len {
                    let Some(x) = self.byte() else {
                        return Ok(None);
                    };
                    hdr.push(x);
                }
            }
            for flag in [8u8, 16] {
                if flags & flag != 0 {
                    loop {
                        let Some(x) = self.byte() else {
                            return Ok(None);
                        };
                        hdr.push(x);
                        if x == 0 {
                            break;
                        }
                    }
                }
            }
            if flags & 2 != 0 {
                let (Some(a), Some(b)) = (self.byte(), self.byte()) else {
                    return Ok(None);
                };
                if u16::from_le_bytes([a, b]) != (crc32(0, &hdr) & 0xffff) as u16 {
                    return Err(data("header crc mismatch"));
                }
            }
            self.check = 0;
        } else {
            if !self.need(16) {
                return Ok(None);
            }
            let cmf = self.take(8);
            let flg = self.take(8);
            if !((cmf << 8) + flg).is_multiple_of(31) {
                return Err(data("incorrect header check"));
            }
            if cmf & 0x0f != 8 {
                return Err(data("unknown compression method"));
            }
            let len = (cmf >> 4) + 8;
            if len > 15 || len > self.window_bits {
                return Err(data("invalid window size"));
            }
            self.check = 1;
            if flg & 0x20 != 0 {
                let mut id = 0u32;
                for _ in 0..4 {
                    let Some(b) = self.byte() else {
                        return Ok(None);
                    };
                    id = (id << 8) | b as u32;
                }
                self.dict_id = Some(id);
                self.mode = Mode::BlockHeader;
                return Ok(Some(()));
            }
        }
        self.mode = Mode::BlockHeader;
        Ok(Some(()))
    }

    fn block_header(&mut self) -> Result<Option<()>, ZError> {
        let Some(last) = self.bits(1) else {
            return Ok(None);
        };
        let Some(kind) = self.bits(2) else {
            return Ok(None);
        };
        match kind {
            0 => {
                self.align();
                let (Some(len), Some(nlen)) = (self.bits(16), self.bits(16)) else {
                    return Ok(None);
                };
                if len != (!nlen & 0xffff) {
                    return Err(data("invalid stored block lengths"));
                }
                self.last = last == 1;
                self.mode = Mode::Stored(len as usize);
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                self.lit = Some(Huffman::new(&l, false).expect("fixed"));
                self.dist = Some(Huffman::new(&[5u8; 32], false).expect("fixed"));
                self.last = last == 1;
                self.mode = Mode::Codes;
            }
            2 => {
                let (Some(nlen), Some(ndist), Some(ncode)) =
                    (self.bits(5), self.bits(5), self.bits(4))
                else {
                    return Ok(None);
                };
                let nlen = nlen as usize + 257;
                let ndist = ndist as usize + 1;
                let ncode = ncode as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err(data("too many length or distance symbols"));
                }
                let mut lengths = [0u8; 19];
                for &o in ORDER.iter().take(ncode) {
                    let Some(v) = self.bits(3) else {
                        return Ok(None);
                    };
                    lengths[o] = v as u8;
                }
                let lencode =
                    Huffman::new(&lengths, true).map_err(|_| data("invalid code lengths set"))?;
                let mut lens = vec![0u8; nlen + ndist];
                let mut index = 0;
                while index < nlen + ndist {
                    let sym = match self.decode(&lencode) {
                        Ok(Some(s)) => s,
                        Ok(None) => return Ok(None),
                        Err(_) => return Err(data("invalid code lengths set")),
                    };
                    if sym < 16 {
                        lens[index] = sym as u8;
                        index += 1;
                    } else {
                        let (len, rep) = match sym {
                            16 => {
                                if index == 0 {
                                    return Err(data("invalid bit length repeat"));
                                }
                                let Some(r) = self.bits(2) else {
                                    return Ok(None);
                                };
                                (lens[index - 1], 3 + r as usize)
                            }
                            17 => {
                                let Some(r) = self.bits(3) else {
                                    return Ok(None);
                                };
                                (0, 3 + r as usize)
                            }
                            _ => {
                                let Some(r) = self.bits(7) else {
                                    return Ok(None);
                                };
                                (0, 11 + r as usize)
                            }
                        };
                        if index + rep > nlen + ndist {
                            return Err(data("invalid bit length repeat"));
                        }
                        for _ in 0..rep {
                            lens[index] = len;
                            index += 1;
                        }
                    }
                }
                if lens[256] == 0 {
                    return Err(data("invalid code -- missing end-of-block"));
                }
                let lit = Huffman::new(&lens[..nlen], false)
                    .map_err(|_| data("invalid literal/lengths set"))?;
                let dist = Huffman::new(&lens[nlen..], false)
                    .map_err(|_| data("invalid distances set"))?;
                self.lit = Some(lit);
                self.dist = Some(dist);
                self.last = last == 1;
                self.mode = Mode::Codes;
            }
            _ => return Err(data("invalid block type")),
        }
        Ok(Some(()))
    }

    /// Decodes symbols of the current block. `Ok(None)` at end of block.
    fn codes(
        &mut self,
        out: &mut Vec<u8>,
        start: usize,
        max_output: usize,
    ) -> Result<Option<Progress>, ZError> {
        let lit = self.lit.take().expect("block tables");
        let dist = self.dist.take().expect("block tables");
        let r = self.codes_inner(out, start, max_output, &lit, &dist);
        self.lit = Some(lit);
        self.dist = Some(dist);
        r
    }

    fn codes_inner(
        &mut self,
        out: &mut Vec<u8>,
        start: usize,
        max_output: usize,
        lit: &Huffman,
        dist: &Huffman,
    ) -> Result<Option<Progress>, ZError> {
        loop {
            if let Some((mut len, d)) = self.copy.take() {
                while len > 0 {
                    if out.len() - start >= max_output {
                        self.copy = Some((len, d));
                        return Ok(Some(Progress::OutputFull));
                    }
                    let b = self.window[self.window.len() - d];
                    self.emit(out, b);
                    len -= 1;
                }
                continue;
            }
            if out.len() - start >= max_output {
                return Ok(Some(Progress::OutputFull));
            }
            let save = self.b;
            let sym = match self.decode(lit) {
                Ok(Some(s)) => s as usize,
                Ok(None) => return Ok(Some(Progress::NeedInput)),
                Err(_) => return Err(data("invalid literal/length code")),
            };
            if sym < 256 {
                self.emit(out, sym as u8);
            } else if sym == 256 {
                return Ok(None);
            } else {
                let li = sym - 257;
                if li >= 29 {
                    return Err(data("invalid literal/length code"));
                }
                let Some(e) = self.bits(LEN_EXTRA[li] as u32) else {
                    self.b = save;
                    return Ok(Some(Progress::NeedInput));
                };
                let len = LEN_BASE[li] as usize + e as usize;
                let ds = match self.decode(dist) {
                    Ok(Some(s)) => s as usize,
                    Ok(None) => {
                        self.b = save;
                        return Ok(Some(Progress::NeedInput));
                    }
                    Err(_) => return Err(data("invalid distance code")),
                };
                if ds >= 30 {
                    return Err(data("invalid distance code"));
                }
                let Some(e) = self.bits(DIST_EXTRA[ds] as u32) else {
                    self.b = save;
                    return Ok(Some(Progress::NeedInput));
                };
                let d = DIST_BASE[ds] as usize + e as usize;
                if d > self.window.len() || d > (1usize << self.window_bits) {
                    return Err(data("invalid distance too far back"));
                }
                self.copy = Some((len, d));
            }
        }
    }
}
