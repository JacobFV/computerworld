//! A port of zlib's `deflate.c` (1.3.1) that produces the library's exact bytes.
//!
//! The state machine is kept as zlib has it — `deflate(strm, flush)` over an
//! input and an output window of a given size, the same window sliding, hash
//! chains, lazy matching and block flushing — because output depends on how a
//! caller drives it (input chunking, flush modes, and for stored blocks the size
//! of the output buffer). Callers that mimic CPython's or Node's buffer strategy
//! therefore get the bytes those programs get.
//!
//! [`HashVariant::Chromium`] reproduces the zlib Node.js ships (Chromium's fork):
//! strings are hashed four bytes at a time with a multiplicative hash, the hash
//! table has at least 2^15 entries, and the window starts zeroed. Everything else
//! is the canonical algorithm.
use crate::trees::{
    tables, Tree, BL_CODES, D_CODES, HEAP_SIZE, L_CODES, MAX_BITS, MAX_MATCH, MIN_MATCH,
};
use crate::{adler32, crc32, Flush, Status, Strategy, ZError};

const NIL: u32 = 0;
const TOO_FAR: usize = 4096;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
const WIN_INIT: usize = MAX_MATCH;
const MAX_STORED: usize = 65535;
const PRESET_DICT: u32 = 0x20;
const OS_CODE: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashVariant {
    /// zlib's Rabin-Karp rolling hash over three bytes (CPython, system zlib).
    Canonical,
    /// Chromium's four-byte multiplicative hash (Node.js's bundled zlib).
    Chromium,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status2 {
    Init,
    Gzip,
    Extra,
    Name,
    Comment,
    Hcrc,
    Busy,
    Finish,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockState {
    NeedMore,
    BlockDone,
    FinishStarted,
    FinishDone,
}

#[derive(Clone, Copy)]
struct Config {
    good: usize,
    lazy: usize,
    nice: usize,
    chain: usize,
    func: u8,
}
const STORED: u8 = 0;
const FAST: u8 = 1;
const SLOW: u8 = 2;
const CONFIG: [Config; 10] = [
    Config {
        good: 0,
        lazy: 0,
        nice: 0,
        chain: 0,
        func: STORED,
    },
    Config {
        good: 4,
        lazy: 4,
        nice: 8,
        chain: 4,
        func: FAST,
    },
    Config {
        good: 4,
        lazy: 5,
        nice: 16,
        chain: 8,
        func: FAST,
    },
    Config {
        good: 4,
        lazy: 6,
        nice: 32,
        chain: 32,
        func: FAST,
    },
    Config {
        good: 4,
        lazy: 4,
        nice: 16,
        chain: 16,
        func: SLOW,
    },
    Config {
        good: 8,
        lazy: 16,
        nice: 32,
        chain: 32,
        func: SLOW,
    },
    Config {
        good: 8,
        lazy: 16,
        nice: 128,
        chain: 128,
        func: SLOW,
    },
    Config {
        good: 8,
        lazy: 32,
        nice: 128,
        chain: 256,
        func: SLOW,
    },
    Config {
        good: 32,
        lazy: 128,
        nice: 258,
        chain: 1024,
        func: SLOW,
    },
    Config {
        good: 32,
        lazy: 258,
        nice: 258,
        chain: 4096,
        func: SLOW,
    },
];

/// A gzip header to write instead of zlib's default one (`deflateSetHeader`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GzHeader {
    pub text: bool,
    pub time: u32,
    pub os: u8,
    pub extra: Option<Vec<u8>>,
    pub name: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    pub hcrc: bool,
}

/// The compressor. Fields mirror `deflate_state` plus the parts of `z_stream`
/// deflate touches.
#[derive(Clone)]
pub struct State {
    // z_stream
    input: Vec<u8>,
    in_pos: usize,
    avail_out: usize,
    out: Vec<u8>,
    pub total_in: u64,
    pub total_out: u64,
    pub adler: u32,
    // deflate_state
    status: Status2,
    pub(crate) pending_buf: Vec<u8>,
    pending_out: usize,
    pending_buf_size: usize,
    wrap: i32,
    gzhead: Option<GzHeader>,
    gzindex: usize,
    last_flush: i32,
    w_size: usize,
    w_bits: u32,
    w_mask: usize,
    pub(crate) window: Vec<u8>,
    window_size: usize,
    prev: Vec<u16>,
    head: Vec<u16>,
    ins_h: u32,

    hash_bits: u32,
    hash_mask: u32,
    hash_shift: u32,
    block_start: i64,
    match_length: usize,
    prev_match: u32,
    match_available: bool,
    strstart: usize,
    match_start: usize,
    lookahead: usize,
    prev_length: usize,
    max_chain_length: usize,
    max_lazy_match: usize,
    pub(crate) level: i32,
    pub(crate) strategy: i32,
    good_match: usize,
    nice_match: usize,
    pub(crate) dyn_ltree: Tree,
    pub(crate) dyn_dtree: Tree,
    pub(crate) bl_tree: Tree,
    pub(crate) bl_count: [u16; MAX_BITS + 1],
    pub(crate) heap: Vec<u16>,
    pub(crate) heap_len: usize,
    pub(crate) heap_max: usize,
    pub(crate) depth: Vec<u8>,
    lit_bufsize: usize,
    pub(crate) sym_d: Vec<u16>,
    pub(crate) sym_l: Vec<u8>,
    pub(crate) sym_end: usize,
    pub(crate) opt_len: u64,
    pub(crate) static_len: u64,
    pub(crate) matches: u32,
    insert: usize,
    pub(crate) bi_buf: u16,
    pub(crate) bi_valid: i32,
    high_water: usize,
    hash: HashVariant,
}

impl State {
    /// `deflateInit2`: `window_bits` 8..15 (zlib wrapper), -8..-15 (raw) or
    /// 24..31 (gzip).
    pub fn new(
        level: i32,
        window_bits: i32,
        mem_level: i32,
        strategy: i32,
        hash: HashVariant,
    ) -> Result<Self, ZError> {
        let mut level = level;
        if level == -1 {
            level = 6;
        }
        let mut wrap = 1;
        let mut wb = window_bits;
        if wb < 0 {
            wrap = 0;
            if wb < -15 {
                return Err(ZError::Stream);
            }
            wb = -wb;
        } else if wb > 15 {
            wrap = 2;
            wb -= 16;
        }
        if !(1..=9).contains(&mem_level)
            || !(8..=15).contains(&wb)
            || !(0..=9).contains(&level)
            || !(0..=4).contains(&strategy)
            || (wb == 8 && wrap != 1)
        {
            return Err(ZError::Stream);
        }
        if wb == 8 {
            wb = 9;
        }
        let w_bits = wb as u32;
        let w_size = 1usize << w_bits;
        let mut hash_bits = mem_level as u32 + 7;
        if hash == HashVariant::Chromium && hash_bits < 15 {
            hash_bits = 15;
        }
        let hash_size = 1usize << hash_bits;
        let lit_bufsize = 1usize << (mem_level + 6);
        let mut s = State {
            input: vec![],
            in_pos: 0,
            avail_out: 0,
            out: vec![],
            total_in: 0,
            total_out: 0,
            adler: 1,
            status: Status2::Init,
            pending_buf: Vec::with_capacity(lit_bufsize * 4),
            pending_out: 0,
            pending_buf_size: lit_bufsize * 4,
            wrap,
            gzhead: None,
            gzindex: 0,
            last_flush: -2,
            w_size,
            w_bits,
            w_mask: w_size - 1,
            window: vec![0; 2 * w_size + 8],
            window_size: 2 * w_size,
            prev: vec![0; w_size],
            head: vec![0; hash_size],
            ins_h: 0,

            hash_bits,
            hash_mask: hash_size as u32 - 1,
            hash_shift: hash_bits.div_ceil(MIN_MATCH as u32),
            block_start: 0,
            match_length: 0,
            prev_match: 0,
            match_available: false,
            strstart: 0,
            match_start: 0,
            lookahead: 0,
            prev_length: 0,
            max_chain_length: 0,
            max_lazy_match: 0,
            level,
            strategy,
            good_match: 0,
            nice_match: 0,
            dyn_ltree: Tree::new(HEAP_SIZE),
            dyn_dtree: Tree::new(2 * D_CODES + 1),
            bl_tree: Tree::new(2 * BL_CODES + 1),
            bl_count: [0; MAX_BITS + 1],
            heap: vec![0; 2 * L_CODES + 1],
            heap_len: 0,
            heap_max: 0,
            depth: vec![0; 2 * L_CODES + 1],
            lit_bufsize,
            sym_d: Vec::with_capacity(lit_bufsize),
            sym_l: Vec::with_capacity(lit_bufsize),
            sym_end: lit_bufsize - 1,
            opt_len: 0,
            static_len: 0,
            matches: 0,
            insert: 0,
            bi_buf: 0,
            bi_valid: 0,
            high_water: 0,
            hash,
        };
        let _ = s.hash_bits;
        let _ = s.lit_bufsize;
        s.reset();
        Ok(s)
    }

    /// `deflateReset`.
    pub fn reset(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.pending_buf.clear();
        self.pending_out = 0;
        if self.wrap < 0 {
            self.wrap = -self.wrap;
        }
        self.status = if self.wrap == 2 {
            Status2::Gzip
        } else {
            Status2::Init
        };
        self.adler = if self.wrap == 2 { 0 } else { 1 };
        self.last_flush = -2;
        self.tr_init();
        // lm_init
        self.window_size = 2 * self.w_size;
        self.clear_hash();
        let c = CONFIG[self.level as usize];
        self.max_lazy_match = c.lazy;
        self.good_match = c.good;
        self.nice_match = c.nice;
        self.max_chain_length = c.chain;
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = 0;
        self.insert = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.match_available = false;
        self.ins_h = 0;
    }

    pub fn set_header(&mut self, h: GzHeader) -> Result<(), ZError> {
        if self.wrap != 2 {
            return Err(ZError::Stream);
        }
        self.gzhead = Some(h);
        Ok(())
    }

    fn clear_hash(&mut self) {
        for h in self.head.iter_mut() {
            *h = 0;
        }
    }

    fn slide_hash(&mut self) {
        let wsize = self.w_size as u32;
        for h in self.head.iter_mut() {
            let m = *h as u32;
            *h = if m >= wsize { (m - wsize) as u16 } else { 0 };
        }
        for p in self.prev.iter_mut() {
            let m = *p as u32;
            *p = if m >= wsize { (m - wsize) as u16 } else { 0 };
        }
    }

    #[inline]
    fn update_hash(&mut self, c: u8) {
        self.ins_h = ((self.ins_h << self.hash_shift) ^ c as u32) & self.hash_mask;
    }

    /// `insert_string`: adds the string at `str` to its hash chain and returns
    /// the previous head.
    #[inline]
    fn insert_string(&mut self, s: usize) -> u32 {
        match self.hash {
            HashVariant::Canonical => {
                let c = self.window[s + MIN_MATCH - 1];
                self.update_hash(c);
            }
            HashVariant::Chromium => {
                let v = u32::from_le_bytes([
                    self.window[s],
                    self.window[s + 1],
                    self.window[s + 2],
                    self.window[s + 3],
                ]);
                self.ins_h = (v.wrapping_mul(66521).wrapping_add(66521) >> 16) & self.hash_mask;
            }
        }
        let h = self.ins_h as usize;
        let ret = self.head[h];
        self.prev[s & self.w_mask] = ret;
        self.head[h] = s as u16;
        ret as u32
    }

    fn max_dist(&self) -> usize {
        self.w_size - MIN_LOOKAHEAD
    }

    fn avail_in(&self) -> usize {
        self.input.len() - self.in_pos
    }

    /// `read_buf`: copies input into `window[at..]` and updates the checksum.
    fn read_buf_window(&mut self, at: usize, size: usize) -> usize {
        let len = self.avail_in().min(size);
        if len == 0 {
            return 0;
        }
        let src = self.in_pos;
        self.window[at..at + len].copy_from_slice(&self.input[src..src + len]);
        self.checksum_input(src, len);
        len
    }

    fn checksum_input(&mut self, src: usize, len: usize) {
        if self.wrap == 1 {
            self.adler = adler32(self.adler, &self.input[src..src + len]);
        } else if self.wrap == 2 {
            self.adler = crc32(self.adler, &self.input[src..src + len]);
        }
        self.in_pos += len;
        self.total_in += len as u64;
    }

    fn fill_window(&mut self) {
        let wsize = self.w_size;
        loop {
            let mut more = self.window_size - self.lookahead - self.strstart;
            if self.strstart >= wsize + self.max_dist() {
                self.window.copy_within(wsize..wsize + wsize - more, 0);
                self.match_start = self.match_start.wrapping_sub(wsize);
                self.strstart -= wsize;
                self.block_start -= wsize as i64;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += wsize;
            }
            if self.avail_in() == 0 {
                break;
            }
            let at = self.strstart + self.lookahead;
            let n = self.read_buf_window(at, more);
            self.lookahead += n;
            match self.hash {
                HashVariant::Chromium => {
                    if self.lookahead + self.insert > MIN_MATCH {
                        let mut s = self.strstart - self.insert;
                        while self.insert > 0 {
                            self.insert_string(s);
                            s += 1;
                            self.insert -= 1;
                            if self.lookahead + self.insert <= MIN_MATCH {
                                break;
                            }
                        }
                    }
                }
                HashVariant::Canonical => {
                    if self.lookahead + self.insert >= MIN_MATCH {
                        let mut s = self.strstart - self.insert;
                        self.ins_h = self.window[s] as u32;
                        let c = self.window[s + 1];
                        self.update_hash(c);
                        while self.insert > 0 {
                            let c = self.window[s + MIN_MATCH - 1];
                            self.update_hash(c);
                            self.prev[s & self.w_mask] = self.head[self.ins_h as usize];
                            self.head[self.ins_h as usize] = s as u16;
                            s += 1;
                            self.insert -= 1;
                            if self.lookahead + self.insert < MIN_MATCH {
                                break;
                            }
                        }
                    }
                }
            }
            if !(self.lookahead < MIN_LOOKAHEAD && self.avail_in() != 0) {
                break;
            }
        }
        if self.high_water < self.window_size {
            let curr = self.strstart + self.lookahead;
            if self.high_water < curr {
                let init = (self.window_size - curr).min(WIN_INIT);
                for b in &mut self.window[curr..curr + init] {
                    *b = 0;
                }
                self.high_water = curr + init;
            } else if self.high_water < curr + WIN_INIT {
                let init =
                    (curr + WIN_INIT - self.high_water).min(self.window_size - self.high_water);
                let hw = self.high_water;
                for b in &mut self.window[hw..hw + init] {
                    *b = 0;
                }
                self.high_water += init;
            }
        }
    }

    /// `deflateSetDictionary`.
    pub fn set_dictionary(&mut self, dict: &[u8]) -> Result<(), ZError> {
        let wrap = self.wrap;
        if wrap == 2 || (wrap == 1 && self.status != Status2::Init) || self.lookahead != 0 {
            return Err(ZError::Stream);
        }
        if wrap == 1 {
            self.adler = adler32(self.adler, dict);
        }
        self.wrap = 0;
        let mut dict = dict;
        if dict.len() >= self.w_size {
            if wrap == 0 {
                self.clear_hash();
                self.strstart = 0;
                self.block_start = 0;
                self.insert = 0;
            }
            dict = &dict[dict.len() - self.w_size..];
        }
        let saved_input = std::mem::replace(&mut self.input, dict.to_vec());
        let saved_pos = std::mem::replace(&mut self.in_pos, 0);
        let saved_total = self.total_in;
        self.fill_window();
        while self.lookahead >= MIN_MATCH {
            let mut s = self.strstart;
            let mut n = self.lookahead - (MIN_MATCH - 1);
            loop {
                self.insert_string(s);
                s += 1;
                n -= 1;
                if n == 0 {
                    break;
                }
            }
            self.strstart = s;
            self.lookahead = MIN_MATCH - 1;
            self.fill_window();
        }
        self.strstart += self.lookahead;
        self.block_start = self.strstart as i64;
        self.insert = self.lookahead;
        self.lookahead = 0;
        self.match_length = MIN_MATCH - 1;
        self.prev_length = MIN_MATCH - 1;
        self.match_available = false;
        self.input = saved_input;
        self.in_pos = saved_pos;
        self.total_in = saved_total;
        self.wrap = wrap;
        Ok(())
    }

    /// `deflateParams`.
    pub fn params(&mut self, level: i32, strategy: i32) -> Result<Vec<u8>, ZError> {
        let level = if level == -1 { 6 } else { level };
        if !(0..=9).contains(&level) || !(0..=4).contains(&strategy) {
            return Err(ZError::Stream);
        }
        let mut out = vec![];
        let func = CONFIG[self.level as usize].func;
        if (strategy != self.strategy || func != CONFIG[level as usize].func)
            && self.last_flush != -2
        {
            // Flush the last buffer (the caller supplies ample output space).
            let (o, _) = self.deflate(&[], usize::MAX / 2, Flush::Block)?;
            out = o;
            if (self.strstart as i64 - self.block_start) as usize + self.lookahead != 0 {
                return Err(ZError::Buf);
            }
        }
        if self.level != level {
            if self.level == 0 && self.matches != 0 {
                if self.matches == 1 {
                    self.slide_hash();
                } else {
                    self.clear_hash();
                }
                self.matches = 0;
            }
            self.level = level;
            let c = CONFIG[level as usize];
            self.max_lazy_match = c.lazy;
            self.good_match = c.good;
            self.nice_match = c.nice;
            self.max_chain_length = c.chain;
        }
        self.strategy = strategy;
        Ok(out)
    }

    fn put_short_msb(&mut self, b: u32) {
        self.put_byte((b >> 8) as u8);
        self.put_byte((b & 0xff) as u8);
    }

    fn pending(&self) -> usize {
        self.pending_buf.len() - self.pending_out
    }

    fn flush_pending(&mut self) {
        self.tr_flush_bits();
        let len = self.pending().min(self.avail_out);
        if len == 0 {
            return;
        }
        let from = self.pending_out;
        self.out
            .extend_from_slice(&self.pending_buf[from..from + len]);
        self.pending_out += len;
        self.total_out += len as u64;
        self.avail_out -= len;
        if self.pending() == 0 {
            self.pending_buf.clear();
            self.pending_out = 0;
        }
    }

    fn hcrc_update(&mut self, beg: usize) {
        if self.gzhead.as_ref().is_some_and(|h| h.hcrc) && self.pending_buf.len() > beg {
            self.adler = crc32(self.adler, &self.pending_buf[beg..]);
        }
    }

    fn rank(f: i32) -> i32 {
        f * 2 - if f > 4 { 9 } else { 0 }
    }

    /// `deflate(strm, flush)` with `input` as `next_in` and room for `avail_out`
    /// bytes of output. Returns the output produced and the status; all of
    /// `input` that zlib would consume is consumed (the count is `input.len()`
    /// minus [`State::unconsumed`]).
    pub fn deflate(
        &mut self,
        input: &[u8],
        avail_out: usize,
        flush: Flush,
    ) -> Result<(Vec<u8>, Status), ZError> {
        self.input = input.to_vec();
        self.in_pos = 0;
        self.avail_out = avail_out;
        self.out = Vec::new();
        let r = self.deflate_inner(flush as i32);
        let out = std::mem::take(&mut self.out);
        r.map(|s| (out, s))
    }

    /// Input bytes the last `deflate` call left unconsumed.
    pub fn unconsumed(&self) -> usize {
        self.avail_in()
    }

    fn deflate_inner(&mut self, flush: i32) -> Result<Status, ZError> {
        if !(0..=5).contains(&flush) {
            return Err(ZError::Stream);
        }
        if self.status == Status2::Finish && flush != Flush::Finish as i32 {
            return Err(ZError::Stream);
        }
        if self.avail_out == 0 {
            return Err(ZError::Buf);
        }
        let old_flush = self.last_flush;
        self.last_flush = flush;
        if self.pending() != 0 {
            self.flush_pending();
            if self.avail_out == 0 {
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        } else if self.avail_in() == 0
            && Self::rank(flush) <= Self::rank(old_flush)
            && flush != Flush::Finish as i32
        {
            return Err(ZError::Buf);
        }
        if self.status == Status2::Finish && self.avail_in() != 0 {
            return Err(ZError::Buf);
        }
        if self.status == Status2::Init && self.wrap == 0 {
            self.status = Status2::Busy;
        }
        if self.status == Status2::Init {
            let mut header = (8 + ((self.w_bits - 8) << 4)) << 8;
            let level_flags = if self.strategy >= Strategy::HuffmanOnly as i32 || self.level < 2 {
                0
            } else if self.level < 6 {
                1
            } else if self.level == 6 {
                2
            } else {
                3
            };
            header |= level_flags << 6;
            if self.strstart != 0 {
                header |= PRESET_DICT;
            }
            header += 31 - (header % 31);
            self.put_short_msb(header);
            if self.strstart != 0 {
                let a = self.adler;
                self.put_short_msb(a >> 16);
                self.put_short_msb(a & 0xffff);
            }
            self.adler = 1;
            self.status = Status2::Busy;
            self.flush_pending();
            if self.pending() != 0 {
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        }
        if self.status == Status2::Gzip {
            self.adler = 0;
            self.put_byte(31);
            self.put_byte(139);
            self.put_byte(8);
            let xfl = if self.level == 9 {
                2
            } else if self.strategy >= Strategy::HuffmanOnly as i32 || self.level < 2 {
                4
            } else {
                0
            };
            match self.gzhead.clone() {
                None => {
                    for _ in 0..5 {
                        self.put_byte(0);
                    }
                    self.put_byte(xfl);
                    self.put_byte(OS_CODE);
                    self.status = Status2::Busy;
                    self.flush_pending();
                    if self.pending() != 0 {
                        self.last_flush = -1;
                        return Ok(Status::Ok);
                    }
                }
                Some(h) => {
                    let flags = u8::from(h.text)
                        | if h.hcrc { 2 } else { 0 }
                        | if h.extra.is_some() { 4 } else { 0 }
                        | if h.name.is_some() { 8 } else { 0 }
                        | if h.comment.is_some() { 16 } else { 0 };
                    self.put_byte(flags);
                    for i in 0..4 {
                        self.put_byte((h.time >> (8 * i)) as u8);
                    }
                    self.put_byte(xfl);
                    self.put_byte(h.os);
                    if let Some(e) = &h.extra {
                        self.put_byte(e.len() as u8);
                        self.put_byte((e.len() >> 8) as u8);
                    }
                    if h.hcrc {
                        self.adler = crc32(self.adler, &self.pending_buf[self.pending_out..]);
                    }
                    self.gzindex = 0;
                    self.status = Status2::Extra;
                }
            }
        }
        if self.status == Status2::Extra {
            if let Some(extra) = self.gzhead.as_ref().and_then(|h| h.extra.clone()) {
                let beg = self.pending_buf.len();
                let left = (extra.len() & 0xffff) - self.gzindex;
                // The pending buffer is unbounded here, so the header fits at once.
                let from = self.gzindex;
                self.pending_buf
                    .extend_from_slice(&extra[from..from + left]);
                self.hcrc_update(beg);
                self.gzindex = 0;
            }
            self.status = Status2::Name;
        }
        if self.status == Status2::Name {
            if let Some(name) = self.gzhead.as_ref().and_then(|h| h.name.clone()) {
                let beg = self.pending_buf.len();
                for &b in name.iter().take_while(|b| **b != 0) {
                    self.put_byte(b);
                }
                self.put_byte(0);
                self.hcrc_update(beg);
                self.gzindex = 0;
            }
            self.status = Status2::Comment;
        }
        if self.status == Status2::Comment {
            if let Some(c) = self.gzhead.as_ref().and_then(|h| h.comment.clone()) {
                let beg = self.pending_buf.len();
                for &b in c.iter().take_while(|b| **b != 0) {
                    self.put_byte(b);
                }
                self.put_byte(0);
                self.hcrc_update(beg);
            }
            self.status = Status2::Hcrc;
        }
        if self.status == Status2::Hcrc {
            if self.gzhead.as_ref().is_some_and(|h| h.hcrc) {
                let a = self.adler;
                self.put_byte(a as u8);
                self.put_byte((a >> 8) as u8);
                self.adler = 0;
            }
            self.status = Status2::Busy;
            self.flush_pending();
            if self.pending() != 0 {
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        }
        if self.avail_in() != 0
            || self.lookahead != 0
            || (flush != Flush::None as i32 && self.status != Status2::Finish)
        {
            let bstate = if self.level == 0 {
                self.deflate_stored(flush)
            } else if self.strategy == Strategy::HuffmanOnly as i32 {
                self.deflate_huff(flush)
            } else if self.strategy == Strategy::Rle as i32 {
                self.deflate_rle(flush)
            } else if CONFIG[self.level as usize].func == FAST {
                self.deflate_fast(flush)
            } else {
                self.deflate_slow(flush)
            };
            if bstate == BlockState::FinishStarted || bstate == BlockState::FinishDone {
                self.status = Status2::Finish;
            }
            if bstate == BlockState::NeedMore || bstate == BlockState::FinishStarted {
                if self.avail_out == 0 {
                    self.last_flush = -1;
                }
                return Ok(Status::Ok);
            }
            if bstate == BlockState::BlockDone {
                if flush == Flush::Partial as i32 {
                    self.tr_align();
                } else if flush != Flush::Block as i32 {
                    self.tr_stored_block(None, 0, false);
                    if flush == Flush::Full as i32 {
                        self.clear_hash();
                        if self.lookahead == 0 {
                            self.strstart = 0;
                            self.block_start = 0;
                            self.insert = 0;
                        }
                    }
                }
                self.flush_pending();
                if self.avail_out == 0 {
                    self.last_flush = -1;
                    return Ok(Status::Ok);
                }
            }
        }
        if flush != Flush::Finish as i32 {
            return Ok(Status::Ok);
        }
        if self.wrap <= 0 {
            return Ok(Status::StreamEnd);
        }
        if self.wrap == 2 {
            let a = self.adler;
            let t = self.total_in as u32;
            for i in 0..4 {
                self.put_byte((a >> (8 * i)) as u8);
            }
            for i in 0..4 {
                self.put_byte((t >> (8 * i)) as u8);
            }
        } else {
            let a = self.adler;
            self.put_short_msb(a >> 16);
            self.put_short_msb(a & 0xffff);
        }
        self.flush_pending();
        if self.wrap > 0 {
            self.wrap = -self.wrap;
        }
        Ok(if self.pending() != 0 {
            Status::Ok
        } else {
            Status::StreamEnd
        })
    }

    // ------------------------------------------------------------- matching

    fn longest_match(&mut self, mut cur_match: u32) -> usize {
        let mut chain_length = self.max_chain_length;
        let scan0 = self.strstart;
        let mut best_len = self.prev_length;
        let mut nice_match = self.nice_match;
        let limit: u32 = if self.strstart > self.max_dist() {
            (self.strstart - self.max_dist()) as u32
        } else {
            NIL
        };
        let wmask = self.w_mask;
        let strend = self.strstart + MAX_MATCH;
        let w = &self.window;
        let mut scan_end1 = w[scan0 + best_len - 1];
        let mut scan_end = w[scan0 + best_len];
        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match > self.lookahead {
            nice_match = self.lookahead;
        }
        loop {
            let m = cur_match as usize;
            let skip = w[m + best_len] != scan_end
                || w[m + best_len - 1] != scan_end1
                || w[m] != w[scan0]
                || w[m + 1] != w[scan0 + 1];
            if !skip {
                // scan += 2, match++ (match now at m + 2 after the ++match above)
                let mut scan = scan0 + 2;
                let mut mat = m + 2;
                loop {
                    // Eight comparisons between checks, as in C.
                    let mut stop = false;
                    for _ in 0..8 {
                        scan += 1;
                        mat += 1;
                        if w[scan] != w[mat] {
                            stop = true;
                            break;
                        }
                    }
                    if stop || scan >= strend {
                        break;
                    }
                }
                let len = MAX_MATCH - (strend - scan);
                if len > best_len {
                    self.match_start = cur_match as usize;
                    best_len = len;
                    if len >= nice_match {
                        break;
                    }
                    scan_end1 = w[scan0 + best_len - 1];
                    scan_end = w[scan0 + best_len];
                }
            }
            cur_match = self.prev[m & wmask] as u32;
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        if best_len <= self.lookahead {
            best_len
        } else {
            self.lookahead
        }
    }

    // ------------------------------------------------------------- blocks

    fn flush_block_only(&mut self, last: bool) {
        let buf = if self.block_start >= 0 {
            Some(self.block_start as usize)
        } else {
            None
        };
        let len = (self.strstart as i64 - self.block_start) as usize;
        self.tr_flush_block(buf, len, last);
        self.block_start = self.strstart as i64;
        self.flush_pending();
    }

    /// FLUSH_BLOCK: returns `Some(state)` when the caller must return.
    fn flush_block(&mut self, last: bool) -> Option<BlockState> {
        self.flush_block_only(last);
        if self.avail_out == 0 {
            return Some(if last {
                BlockState::FinishStarted
            } else {
                BlockState::NeedMore
            });
        }
        None
    }

    fn deflate_stored(&mut self, flush: i32) -> BlockState {
        let mut min_block = (self.pending_buf_size - 5).min(self.w_size);
        let mut last = false;
        let used0 = self.avail_in();
        loop {
            let mut len = MAX_STORED;
            let have = ((self.bi_valid + 42) >> 3) as usize;
            if self.avail_out < have {
                break;
            }
            let have = self.avail_out - have;
            let left = (self.strstart as i64 - self.block_start) as usize;
            if len > left + self.avail_in() {
                len = left + self.avail_in();
            }
            if len > have {
                len = have;
            }
            if len < min_block
                && ((len == 0 && flush != Flush::Finish as i32)
                    || flush == Flush::None as i32
                    || len != left + self.avail_in())
            {
                break;
            }
            last = flush == Flush::Finish as i32 && len == left + self.avail_in();
            self.tr_stored_block(None, 0, last);
            let n = self.pending_buf.len();
            self.pending_buf[n - 4] = len as u8;
            self.pending_buf[n - 3] = (len >> 8) as u8;
            self.pending_buf[n - 2] = !len as u8;
            self.pending_buf[n - 1] = (!len >> 8) as u8;
            self.flush_pending();
            if left > 0 {
                let l = left.min(len);
                let from = self.block_start as usize;
                self.out.extend_from_slice(&self.window[from..from + l]);
                self.avail_out -= l;
                self.total_out += l as u64;
                self.block_start += l as i64;
                len -= l;
            }
            if len > 0 {
                let src = self.in_pos;
                let data = self.input[src..src + len].to_vec();
                self.checksum_input(src, len);
                self.out.extend_from_slice(&data);
                self.avail_out -= len;
                self.total_out += len as u64;
            }
            if last {
                break;
            }
        }
        let used = used0 - self.avail_in();
        if used > 0 {
            if used >= self.w_size {
                self.matches = 2;
                let end = self.in_pos;
                let ws = self.w_size;
                let src = self.input[end - ws..end].to_vec();
                self.window[..ws].copy_from_slice(&src);
                self.strstart = self.w_size;
                self.insert = self.strstart;
            } else {
                if self.window_size - self.strstart <= used {
                    self.strstart -= self.w_size;
                    let ws = self.w_size;
                    let ss = self.strstart;
                    self.window.copy_within(ws..ws + ss, 0);
                    if self.matches < 2 {
                        self.matches += 1;
                    }
                    if self.insert > self.strstart {
                        self.insert = self.strstart;
                    }
                }
                let end = self.in_pos;
                let ss = self.strstart;
                let src = self.input[end - used..end].to_vec();
                self.window[ss..ss + used].copy_from_slice(&src);
                self.strstart += used;
                self.insert += used.min(self.w_size - self.insert);
            }
            self.block_start = self.strstart as i64;
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }
        if last {
            return BlockState::FinishDone;
        }
        if flush != Flush::None as i32
            && flush != Flush::Finish as i32
            && self.avail_in() == 0
            && self.strstart as i64 == self.block_start
        {
            return BlockState::BlockDone;
        }
        let mut have = self.window_size - self.strstart;
        if self.avail_in() > have && self.block_start >= self.w_size as i64 {
            self.block_start -= self.w_size as i64;
            self.strstart -= self.w_size;
            let ws = self.w_size;
            let ss = self.strstart;
            self.window.copy_within(ws..ws + ss, 0);
            if self.matches < 2 {
                self.matches += 1;
            }
            have += self.w_size;
            if self.insert > self.strstart {
                self.insert = self.strstart;
            }
        }
        if have > self.avail_in() {
            have = self.avail_in();
        }
        if have > 0 {
            let at = self.strstart;
            self.read_buf_window(at, have);
            self.strstart += have;
            self.insert += have.min(self.w_size - self.insert);
        }
        if self.high_water < self.strstart {
            self.high_water = self.strstart;
        }
        let have = ((self.bi_valid + 42) >> 3) as usize;
        let have = (self.pending_buf_size - have).min(MAX_STORED);
        min_block = have.min(self.w_size);
        let left = (self.strstart as i64 - self.block_start) as usize;
        if left >= min_block
            || ((left > 0 || flush == Flush::Finish as i32)
                && flush != Flush::None as i32
                && self.avail_in() == 0
                && left <= have)
        {
            let len = left.min(have);
            last = flush == Flush::Finish as i32 && self.avail_in() == 0 && len == left;
            let bs = self.block_start as usize;
            self.tr_stored_block(Some((bs, len)), len, last);
            self.block_start += len as i64;
            self.flush_pending();
        }
        if last {
            BlockState::FinishStarted
        } else {
            BlockState::NeedMore
        }
    }

    fn deflate_fast(&mut self, flush: i32) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window();
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None as i32 {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            if hash_head != NIL && self.strstart - hash_head as usize <= self.max_dist() {
                self.match_length = self.longest_match(hash_head);
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.tally_dist(
                    self.strstart - self.match_start,
                    self.match_length - MIN_MATCH,
                );
                self.lookahead -= self.match_length;
                if self.match_length <= self.max_lazy_match && self.lookahead >= MIN_MATCH {
                    self.match_length -= 1;
                    loop {
                        self.strstart += 1;
                        self.insert_string(self.strstart);
                        self.match_length -= 1;
                        if self.match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += self.match_length;
                    self.match_length = 0;
                    if self.hash == HashVariant::Canonical {
                        self.ins_h = self.window[self.strstart] as u32;
                        let c = self.window[self.strstart + 1];
                        self.update_hash(c);
                    }
                }
            } else {
                let c = self.window[self.strstart];
                bflush = self.tally_lit(c);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                if let Some(r) = self.flush_block(false) {
                    return r;
                }
            }
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Flush::Finish as i32 {
            if let Some(r) = self.flush_block(true) {
                return r;
            }
            return BlockState::FinishDone;
        }
        if !self.sym_l.is_empty() {
            if let Some(r) = self.flush_block(false) {
                return r;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_slow(&mut self, flush: i32) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD {
                self.fill_window();
                if self.lookahead < MIN_LOOKAHEAD && flush == Flush::None as i32 {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start as u32;
            self.match_length = MIN_MATCH - 1;
            if hash_head != NIL
                && self.prev_length < self.max_lazy_match
                && self.strstart - hash_head as usize <= self.max_dist()
            {
                self.match_length = self.longest_match(hash_head);
                if self.match_length <= 5
                    && (self.strategy == Strategy::Filtered as i32
                        || (self.match_length == MIN_MATCH
                            && self.strstart - self.match_start > TOO_FAR))
                {
                    self.match_length = MIN_MATCH - 1;
                }
            }
            if self.prev_length >= MIN_MATCH && self.match_length <= self.prev_length {
                let max_insert = self.strstart + self.lookahead - MIN_MATCH;
                let bflush = self.tally_dist(
                    self.strstart - 1 - self.prev_match as usize,
                    self.prev_length - MIN_MATCH,
                );
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        self.insert_string(self.strstart);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.match_available = false;
                self.match_length = MIN_MATCH - 1;
                self.strstart += 1;
                if bflush {
                    if let Some(r) = self.flush_block(false) {
                        return r;
                    }
                }
            } else if self.match_available {
                let c = self.window[self.strstart - 1];
                let bflush = self.tally_lit(c);
                if bflush {
                    self.flush_block_only(false);
                }
                self.strstart += 1;
                self.lookahead -= 1;
                if self.avail_out == 0 {
                    return BlockState::NeedMore;
                }
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            let c = self.window[self.strstart - 1];
            self.tally_lit(c);
            self.match_available = false;
        }
        self.insert = if self.strstart < MIN_MATCH - 1 {
            self.strstart
        } else {
            MIN_MATCH - 1
        };
        if flush == Flush::Finish as i32 {
            if let Some(r) = self.flush_block(true) {
                return r;
            }
            return BlockState::FinishDone;
        }
        if !self.sym_l.is_empty() {
            if let Some(r) = self.flush_block(false) {
                return r;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_rle(&mut self, flush: i32) -> BlockState {
        loop {
            if self.lookahead <= MAX_MATCH {
                self.fill_window();
                if self.lookahead <= MAX_MATCH && flush == Flush::None as i32 {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            self.match_length = 0;
            if self.lookahead >= MIN_MATCH && self.strstart > 0 {
                let w = &self.window;
                let mut scan = self.strstart - 1;
                let prev = w[scan];
                if prev == w[scan + 1] && prev == w[scan + 2] && prev == w[scan + 3] {
                    scan += 3;
                    let strend = self.strstart + MAX_MATCH;
                    loop {
                        let mut stop = false;
                        for _ in 0..8 {
                            scan += 1;
                            if w[scan] != prev {
                                stop = true;
                                break;
                            }
                        }
                        if stop || scan >= strend {
                            break;
                        }
                    }
                    self.match_length = MAX_MATCH - (strend - scan);
                    if self.match_length > self.lookahead {
                        self.match_length = self.lookahead;
                    }
                }
            }
            let bflush;
            if self.match_length >= MIN_MATCH {
                bflush = self.tally_dist(1, self.match_length - MIN_MATCH);
                self.lookahead -= self.match_length;
                self.strstart += self.match_length;
                self.match_length = 0;
            } else {
                let c = self.window[self.strstart];
                bflush = self.tally_lit(c);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                if let Some(r) = self.flush_block(false) {
                    return r;
                }
            }
        }
        self.insert = 0;
        if flush == Flush::Finish as i32 {
            if let Some(r) = self.flush_block(true) {
                return r;
            }
            return BlockState::FinishDone;
        }
        if !self.sym_l.is_empty() {
            if let Some(r) = self.flush_block(false) {
                return r;
            }
        }
        BlockState::BlockDone
    }

    fn deflate_huff(&mut self, flush: i32) -> BlockState {
        loop {
            if self.lookahead == 0 {
                self.fill_window();
                if self.lookahead == 0 {
                    if flush == Flush::None as i32 {
                        return BlockState::NeedMore;
                    }
                    break;
                }
            }
            self.match_length = 0;
            let c = self.window[self.strstart];
            let bflush = self.tally_lit(c);
            self.lookahead -= 1;
            self.strstart += 1;
            if bflush {
                if let Some(r) = self.flush_block(false) {
                    return r;
                }
            }
        }
        self.insert = 0;
        if flush == Flush::Finish as i32 {
            if let Some(r) = self.flush_block(true) {
                return r;
            }
            return BlockState::FinishDone;
        }
        if !self.sym_l.is_empty() {
            if let Some(r) = self.flush_block(false) {
                return r;
            }
        }
        BlockState::BlockDone
    }
}

// Silence the unused-constant lint for tables only used through `tables()`.
#[allow(dead_code)]
fn _unused() {
    let _ = tables();
}
