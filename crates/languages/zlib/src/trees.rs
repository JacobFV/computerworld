//! Huffman trees and block output: a line-by-line port of zlib's `trees.c`
//! (1.3.1). Tree construction, tie-breaking and the stored/fixed/dynamic block
//! choice follow the C code exactly, so the bit stream is the library's.
use crate::deflate::State;
use std::sync::OnceLock;

pub const LENGTH_CODES: usize = 29;
pub const LITERALS: usize = 256;
pub const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
pub const D_CODES: usize = 30;
pub const BL_CODES: usize = 19;
pub const HEAP_SIZE: usize = 2 * L_CODES + 1;
pub const MAX_BITS: usize = 15;
const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const BUF_SIZE: i32 = 16;
pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;
const STORED_BLOCK: u32 = 0;
const STATIC_TREES: u32 = 1;
const DYN_TREES: u32 = 2;
const SMALLEST: usize = 1;

const EXTRA_LBITS: [u8; LENGTH_CODES] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const EXTRA_DBITS: [u8; D_CODES] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const EXTRA_BLBITS: [u8; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [usize; BL_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The static trees and code tables `tr_static_init` builds.
pub struct Tables {
    pub static_ltree_code: [u16; L_CODES + 2],
    pub static_ltree_len: [u16; L_CODES + 2],
    pub static_dtree_code: [u16; D_CODES],
    pub static_dtree_len: [u16; D_CODES],
    pub dist_code: [u8; 512],
    pub length_code: [u8; MAX_MATCH - MIN_MATCH + 1],
    pub base_length: [u32; LENGTH_CODES],
    pub base_dist: [u32; D_CODES],
}

fn bi_reverse(mut code: u32, mut len: u32) -> u32 {
    let mut res = 0u32;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len == 0 {
            break;
        }
    }
    res >> 1
}

fn gen_codes(code: &mut [u16], len: &[u16], max_code: usize, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut c: u32 = 0;
    for bits in 1..=MAX_BITS {
        c = (c + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = c as u16;
    }
    for n in 0..=max_code {
        let l = len[n] as usize;
        if l == 0 {
            continue;
        }
        code[n] = bi_reverse(next_code[l] as u32, l as u32) as u16;
        next_code[l] = next_code[l].wrapping_add(1);
    }
}

pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Tables {
            static_ltree_code: [0; L_CODES + 2],
            static_ltree_len: [0; L_CODES + 2],
            static_dtree_code: [0; D_CODES],
            static_dtree_len: [0; D_CODES],
            dist_code: [0; 512],
            length_code: [0; MAX_MATCH - MIN_MATCH + 1],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
        };
        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            t.base_length[code] = length as u32;
            for _ in 0..(1usize << EXTRA_LBITS[code]) {
                t.length_code[length] = code as u8;
                length += 1;
            }
            code += 1;
        }
        t.length_code[length - 1] = code as u8;
        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            t.base_dist[code] = dist as u32;
            for _ in 0..(1usize << EXTRA_DBITS[code]) {
                t.dist_code[dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            t.base_dist[code] = (dist << 7) as u32;
            for _ in 0..(1usize << (EXTRA_DBITS[code] - 7)) {
                t.dist_code[256 + dist] = code as u8;
                dist += 1;
            }
            code += 1;
        }
        let mut bl_count = [0u16; MAX_BITS + 1];
        let mut n = 0;
        while n <= 143 {
            t.static_ltree_len[n] = 8;
            bl_count[8] += 1;
            n += 1;
        }
        while n <= 255 {
            t.static_ltree_len[n] = 9;
            bl_count[9] += 1;
            n += 1;
        }
        while n <= 279 {
            t.static_ltree_len[n] = 7;
            bl_count[7] += 1;
            n += 1;
        }
        while n <= 287 {
            t.static_ltree_len[n] = 8;
            bl_count[8] += 1;
            n += 1;
        }
        let lens = t.static_ltree_len;
        gen_codes(&mut t.static_ltree_code, &lens, L_CODES + 1, &bl_count);
        for n in 0..D_CODES {
            t.static_dtree_len[n] = 5;
            t.static_dtree_code[n] = bi_reverse(n as u32, 5) as u16;
        }
        t
    })
}

#[inline]
pub fn d_code(t: &Tables, dist: usize) -> usize {
    if dist < 256 {
        t.dist_code[dist] as usize
    } else {
        t.dist_code[256 + (dist >> 7)] as usize
    }
}

/// One dynamic tree: frequencies (and, after `gen_codes`, codes), parent links
/// and bit lengths.
#[derive(Clone)]
pub struct Tree {
    pub freq: Vec<u32>,
    pub code: Vec<u16>,
    pub dad: Vec<u16>,
    pub len: Vec<u16>,
    pub max_code: i32,
}
impl Tree {
    pub fn new(n: usize) -> Self {
        Self {
            freq: vec![0; n],
            code: vec![0; n],
            dad: vec![0; n],
            len: vec![0; n],
            max_code: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    L,
    D,
    Bl,
}

impl State {
    // -------------------------------------------------------------- bit output
    #[inline]
    pub fn put_byte(&mut self, b: u8) {
        self.pending_buf.push(b);
    }
    #[inline]
    fn put_short(&mut self, w: u16) {
        self.put_byte((w & 0xff) as u8);
        self.put_byte((w >> 8) as u8);
    }
    #[inline]
    pub fn send_bits(&mut self, value: u32, length: i32) {
        if self.bi_valid > BUF_SIZE - length {
            let val = value as i32;
            self.bi_buf |= ((val as u32) << self.bi_valid) as u16;
            let b = self.bi_buf;
            self.put_short(b);
            self.bi_buf = ((val as u32 & 0xffff) >> (BUF_SIZE - self.bi_valid)) as u16;
            self.bi_valid += length - BUF_SIZE;
        } else {
            self.bi_buf |= (value << self.bi_valid) as u16;
            self.bi_valid += length;
        }
    }
    pub fn bi_flush(&mut self) {
        if self.bi_valid == 16 {
            let b = self.bi_buf;
            self.put_short(b);
            self.bi_buf = 0;
            self.bi_valid = 0;
        } else if self.bi_valid >= 8 {
            let b = self.bi_buf as u8;
            self.put_byte(b);
            self.bi_buf >>= 8;
            self.bi_valid -= 8;
        }
    }
    pub fn bi_windup(&mut self) {
        if self.bi_valid > 8 {
            let b = self.bi_buf;
            self.put_short(b);
        } else if self.bi_valid > 0 {
            let b = self.bi_buf as u8;
            self.put_byte(b);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }

    fn tree(&mut self, w: Which) -> &mut Tree {
        match w {
            Which::L => &mut self.dyn_ltree,
            Which::D => &mut self.dyn_dtree,
            Which::Bl => &mut self.bl_tree,
        }
    }

    // -------------------------------------------------------------- blocks
    pub fn init_block(&mut self) {
        for n in 0..L_CODES {
            self.dyn_ltree.freq[n] = 0;
        }
        for n in 0..D_CODES {
            self.dyn_dtree.freq[n] = 0;
        }
        for n in 0..BL_CODES {
            self.bl_tree.freq[n] = 0;
        }
        self.dyn_ltree.freq[END_BLOCK] = 1;
        self.opt_len = 0;
        self.static_len = 0;
        self.sym_d.clear();
        self.sym_l.clear();
        self.matches = 0;
    }

    pub fn tr_init(&mut self) {
        self.bi_buf = 0;
        self.bi_valid = 0;
        self.init_block();
    }

    #[inline]
    fn smaller(tree: &Tree, depth: &[u8], n: usize, m: usize) -> bool {
        tree.freq[n] < tree.freq[m] || (tree.freq[n] == tree.freq[m] && depth[n] <= depth[m])
    }

    fn pqdownheap(&mut self, w: Which, mut k: usize) {
        let v = self.heap[k];
        let mut j = k << 1;
        while j <= self.heap_len {
            {
                let tree = match w {
                    Which::L => &self.dyn_ltree,
                    Which::D => &self.dyn_dtree,
                    Which::Bl => &self.bl_tree,
                };
                if j < self.heap_len
                    && Self::smaller(
                        tree,
                        &self.depth,
                        self.heap[j + 1] as usize,
                        self.heap[j] as usize,
                    )
                {
                    j += 1;
                }
                if Self::smaller(tree, &self.depth, v as usize, self.heap[j] as usize) {
                    break;
                }
            }
            self.heap[k] = self.heap[j];
            k = j;
            j <<= 1;
        }
        self.heap[k] = v;
    }

    fn gen_bitlen(&mut self, w: Which) {
        let t = tables();
        let (stree_len, extra, base, max_length): (Option<&[u16]>, &[u8], usize, usize) = match w {
            Which::L => (
                Some(&t.static_ltree_len[..]),
                &EXTRA_LBITS[..],
                LITERALS + 1,
                MAX_BITS,
            ),
            Which::D => (Some(&t.static_dtree_len[..]), &EXTRA_DBITS[..], 0, MAX_BITS),
            Which::Bl => (None, &EXTRA_BLBITS[..], 0, MAX_BL_BITS),
        };
        let max_code = self.tree(w).max_code;
        for bits in 0..=MAX_BITS {
            self.bl_count[bits] = 0;
        }
        let root = self.heap[self.heap_max] as usize;
        self.tree(w).len[root] = 0;
        let mut overflow = 0i32;
        let mut h = self.heap_max + 1;
        while h < HEAP_SIZE {
            let n = self.heap[h] as usize;
            let tree = match w {
                Which::L => &mut self.dyn_ltree,
                Which::D => &mut self.dyn_dtree,
                Which::Bl => &mut self.bl_tree,
            };
            let mut bits = tree.len[tree.dad[n] as usize] as usize + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            tree.len[n] = bits as u16;
            h += 1;
            if n as i32 > max_code {
                continue;
            }
            self.bl_count[bits] += 1;
            let mut xbits = 0usize;
            if n >= base {
                xbits = extra[n - base] as usize;
            }
            let f = tree.freq[n] as u64;
            self.opt_len = self.opt_len.wrapping_add(f * (bits + xbits) as u64);
            if let Some(sl) = stree_len {
                self.static_len = self
                    .static_len
                    .wrapping_add(f * (sl[n] as usize + xbits) as u64);
            }
        }
        if overflow == 0 {
            return;
        }
        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits] == 0 {
                bits -= 1;
            }
            self.bl_count[bits] -= 1;
            self.bl_count[bits + 1] += 2;
            self.bl_count[max_length] -= 1;
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }
        let mut h = HEAP_SIZE;
        let mut bits = max_length;
        while bits != 0 {
            let mut n = self.bl_count[bits];
            while n != 0 {
                h -= 1;
                let m = self.heap[h] as usize;
                let tree = match w {
                    Which::L => &mut self.dyn_ltree,
                    Which::D => &mut self.dyn_dtree,
                    Which::Bl => &mut self.bl_tree,
                };
                if m as i32 > max_code {
                    continue;
                }
                if tree.len[m] as usize != bits {
                    // C computes this in unsigned long: ((ulg)bits - Len) * Freq.
                    let delta = (bits as u64).wrapping_sub(tree.len[m] as u64);
                    self.opt_len = self
                        .opt_len
                        .wrapping_add(delta.wrapping_mul(tree.freq[m] as u64));
                    tree.len[m] = bits as u16;
                }
                n -= 1;
            }
            bits -= 1;
        }
    }

    fn build_tree(&mut self, w: Which) {
        let t = tables();
        let (stree_len, elems): (Option<&[u16]>, usize) = match w {
            Which::L => (Some(&t.static_ltree_len[..]), L_CODES),
            Which::D => (Some(&t.static_dtree_len[..]), D_CODES),
            Which::Bl => (None, BL_CODES),
        };
        let mut max_code: i32 = -1;
        self.heap_len = 0;
        self.heap_max = HEAP_SIZE;
        for n in 0..elems {
            if self.tree(w).freq[n] != 0 {
                self.heap_len += 1;
                self.heap[self.heap_len] = n as u16;
                max_code = n as i32;
                self.depth[n] = 0;
            } else {
                self.tree(w).len[n] = 0;
            }
        }
        while self.heap_len < 2 {
            let node = if max_code < 2 {
                max_code += 1;
                max_code as usize
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len] = node as u16;
            self.tree(w).freq[node] = 1;
            self.depth[node] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if let Some(sl) = stree_len {
                self.static_len = self.static_len.wrapping_sub(sl[node] as u64);
            }
        }
        self.tree(w).max_code = max_code;
        let mut n = self.heap_len / 2;
        while n >= 1 {
            self.pqdownheap(w, n);
            n -= 1;
        }
        let mut node = elems;
        loop {
            // pqremove
            let n = self.heap[SMALLEST] as usize;
            self.heap[SMALLEST] = self.heap[self.heap_len];
            self.heap_len -= 1;
            self.pqdownheap(w, SMALLEST);
            let m = self.heap[SMALLEST] as usize;
            self.heap_max -= 1;
            self.heap[self.heap_max] = n as u16;
            self.heap_max -= 1;
            self.heap[self.heap_max] = m as u16;
            let tree = self.tree(w);
            tree.freq[node] = tree.freq[n] + tree.freq[m];
            let dn = self.depth[n];
            let dm = self.depth[m];
            self.depth[node] = (if dn >= dm { dn } else { dm }).wrapping_add(1);
            let tree = self.tree(w);
            tree.dad[n] = node as u16;
            tree.dad[m] = node as u16;
            self.heap[SMALLEST] = node as u16;
            node += 1;
            self.pqdownheap(w, SMALLEST);
            if self.heap_len < 2 {
                break;
            }
        }
        self.heap_max -= 1;
        self.heap[self.heap_max] = self.heap[SMALLEST];
        self.gen_bitlen(w);
        let bl_count = self.bl_count;
        let tree = self.tree(w);
        let lens = tree.len.clone();
        gen_codes(&mut tree.code, &lens, max_code.max(0) as usize, &bl_count);
        if max_code < 0 {
            tree.code[0] = 0;
        }
    }

    fn scan_tree(&mut self, w: Which) {
        let max_code = self.tree(w).max_code;
        let tree_len = {
            let tree = self.tree(w);
            // Guard, as in C (the element's length is reset by the next build).
            if (max_code + 1) as usize >= tree.len.len() {
                tree.len.push(0);
            }
            tree.len[(max_code + 1) as usize] = 0xffff;
            tree.len.clone()
        };
        let mut prevlen: i32 = -1;
        let mut nextlen = tree_len[0] as i32;
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=(max_code as usize) {
            let curlen = nextlen;
            nextlen = tree_len[n + 1] as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                self.bl_tree.freq[curlen as usize] += count as u32;
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.bl_tree.freq[curlen as usize] += 1;
                }
                self.bl_tree.freq[REP_3_6] += 1;
            } else if count <= 10 {
                self.bl_tree.freq[REPZ_3_10] += 1;
            } else {
                self.bl_tree.freq[REPZ_11_138] += 1;
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    #[inline]
    fn send_code_bl(&mut self, c: usize) {
        let code = self.bl_tree.code[c] as u32;
        let len = self.bl_tree.len[c] as i32;
        self.send_bits(code, len);
    }

    fn send_tree(&mut self, w: Which, max_code: i32) {
        let tree_len = self.tree(w).len.clone();
        let mut prevlen: i32 = -1;
        let mut nextlen = tree_len[0] as i32;
        let mut count = 0;
        let mut max_count = 7;
        let mut min_count = 4;
        if nextlen == 0 {
            max_count = 138;
            min_count = 3;
        }
        for n in 0..=(max_code as usize) {
            let curlen = nextlen;
            nextlen = tree_len[n + 1] as i32;
            count += 1;
            if count < max_count && curlen == nextlen {
                continue;
            } else if count < min_count {
                loop {
                    self.send_code_bl(curlen as usize);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            } else if curlen != 0 {
                if curlen != prevlen {
                    self.send_code_bl(curlen as usize);
                    count -= 1;
                }
                self.send_code_bl(REP_3_6);
                self.send_bits((count - 3) as u32, 2);
            } else if count <= 10 {
                self.send_code_bl(REPZ_3_10);
                self.send_bits((count - 3) as u32, 3);
            } else {
                self.send_code_bl(REPZ_11_138);
                self.send_bits((count - 11) as u32, 7);
            }
            count = 0;
            prevlen = curlen;
            if nextlen == 0 {
                max_count = 138;
                min_count = 3;
            } else if curlen == nextlen {
                max_count = 6;
                min_count = 3;
            } else {
                max_count = 7;
                min_count = 4;
            }
        }
    }

    fn build_bl_tree(&mut self) -> usize {
        self.scan_tree(Which::L);
        self.scan_tree(Which::D);
        self.build_tree(Which::Bl);
        let mut max_blindex = BL_CODES - 1;
        while max_blindex >= 3 {
            if self.bl_tree.len[BL_ORDER[max_blindex]] != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.opt_len = self
            .opt_len
            .wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&mut self, lcodes: usize, dcodes: usize, blcodes: usize) {
        self.send_bits((lcodes - 257) as u32, 5);
        self.send_bits((dcodes - 1) as u32, 5);
        self.send_bits((blcodes - 4) as u32, 4);
        for &order in BL_ORDER.iter().take(blcodes) {
            let l = self.bl_tree.len[order] as u32;
            self.send_bits(l, 3);
        }
        self.send_tree(Which::L, lcodes as i32 - 1);
        self.send_tree(Which::D, dcodes as i32 - 1);
    }

    /// `_tr_stored_block`: `buf` is the window range, `None` for an empty marker.
    pub fn tr_stored_block(&mut self, buf: Option<(usize, usize)>, stored_len: usize, last: bool) {
        self.send_bits((STORED_BLOCK << 1) + last as u32, 3);
        self.bi_windup();
        self.put_short(stored_len as u16);
        self.put_short(!(stored_len as u16));
        if stored_len > 0 {
            if let Some((start, _)) = buf {
                let data = self.window[start..start + stored_len].to_vec();
                self.pending_buf.extend_from_slice(&data);
            }
        }
    }

    pub fn tr_flush_bits(&mut self) {
        self.bi_flush();
    }

    pub fn tr_align(&mut self) {
        let t = tables();
        self.send_bits(STATIC_TREES << 1, 3);
        self.send_bits(
            t.static_ltree_code[END_BLOCK] as u32,
            t.static_ltree_len[END_BLOCK] as i32,
        );
        self.bi_flush();
    }

    fn compress_block(&mut self, stat: bool) {
        let t = tables();
        let n = self.sym_l.len();
        let syms_d = std::mem::take(&mut self.sym_d);
        let syms_l = std::mem::take(&mut self.sym_l);
        let lcode = |s: &State, c: usize| -> (u32, i32) {
            if stat {
                (t.static_ltree_code[c] as u32, t.static_ltree_len[c] as i32)
            } else {
                (s.dyn_ltree.code[c] as u32, s.dyn_ltree.len[c] as i32)
            }
        };
        let dcode = |s: &State, c: usize| -> (u32, i32) {
            if stat {
                (t.static_dtree_code[c] as u32, t.static_dtree_len[c] as i32)
            } else {
                (s.dyn_dtree.code[c] as u32, s.dyn_dtree.len[c] as i32)
            }
        };
        for i in 0..n {
            let mut dist = syms_d[i] as usize;
            let mut lc = syms_l[i] as usize;
            if dist == 0 {
                let (c, l) = lcode(self, lc);
                self.send_bits(c, l);
            } else {
                let code = t.length_code[lc] as usize;
                let (c, l) = lcode(self, code + LITERALS + 1);
                self.send_bits(c, l);
                let extra = EXTRA_LBITS[code] as i32;
                if extra != 0 {
                    lc -= t.base_length[code] as usize;
                    self.send_bits(lc as u32, extra);
                }
                dist -= 1;
                let code = d_code(t, dist);
                let (c, l) = dcode(self, code);
                self.send_bits(c, l);
                let extra = EXTRA_DBITS[code] as i32;
                if extra != 0 {
                    dist -= t.base_dist[code] as usize;
                    self.send_bits(dist as u32, extra);
                }
            }
        }
        let (c, l) = lcode(self, END_BLOCK);
        self.send_bits(c, l);
        self.sym_d = syms_d;
        self.sym_l = syms_l;
    }

    /// `_tr_flush_block`: `buf` is `Some((start, len))` in the window, or `None`
    /// when the block's data is no longer in the window.
    pub fn tr_flush_block(&mut self, buf: Option<usize>, stored_len: usize, last: bool) {
        let opt_lenb;
        let static_lenb;
        let mut max_blindex = 0;
        if self.level > 0 {
            self.build_tree(Which::L);
            self.build_tree(Which::D);
            max_blindex = self.build_bl_tree();
            let o = self.opt_len.wrapping_add(3 + 7) >> 3;
            let s = self.static_len.wrapping_add(3 + 7) >> 3;
            static_lenb = s;
            opt_lenb = if s <= o || self.strategy == crate::Strategy::Fixed as i32 {
                s
            } else {
                o
            };
        } else {
            opt_lenb = stored_len as u64 + 5;
            static_lenb = opt_lenb;
        }
        if stored_len as u64 + 4 <= opt_lenb && buf.is_some() {
            self.tr_stored_block(buf.map(|b| (b, stored_len)), stored_len, last);
        } else if static_lenb == opt_lenb {
            self.send_bits((STATIC_TREES << 1) + last as u32, 3);
            self.compress_block(true);
        } else {
            self.send_bits((DYN_TREES << 1) + last as u32, 3);
            let l = self.dyn_ltree.max_code as usize + 1;
            let d = self.dyn_dtree.max_code as usize + 1;
            self.send_all_trees(l, d, max_blindex + 1);
            self.compress_block(false);
        }
        self.init_block();
        if last {
            self.bi_windup();
        }
    }

    #[inline]
    pub fn tally_lit(&mut self, c: u8) -> bool {
        self.sym_d.push(0);
        self.sym_l.push(c);
        self.dyn_ltree.freq[c as usize] += 1;
        self.sym_l.len() == self.sym_end
    }

    #[inline]
    pub fn tally_dist(&mut self, distance: usize, length: usize) -> bool {
        let t = tables();
        self.sym_d.push(distance as u16);
        self.sym_l.push(length as u8);
        let dist = distance - 1;
        self.dyn_ltree.freq[t.length_code[length] as usize + LITERALS + 1] += 1;
        self.dyn_dtree.freq[d_code(t, dist)] += 1;
        self.sym_l.len() == self.sym_end
    }
}
