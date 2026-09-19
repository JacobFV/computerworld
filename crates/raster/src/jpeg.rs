//! Baseline JPEG (ITU-T T.81) encoding: JFIF, 8-bit, Huffman-coded sequential DCT with
//! the standard tables of Annex K, the IJG quality scale and 4:4:4 or 4:2:0 chroma. The
//! transform is integer (a fixed-point DCT matrix built from [`fmath::cos`]), so the
//! same pixels give the same bytes on every target. Transparency is flattened over
//! white, since a JPEG has none.
use crate::fmath;
use crate::{Canvas, Rgba};
use serde::{Deserialize, Serialize};

/// Chroma subsampling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsampling {
    /// Full-resolution colour ("4:4:4 (best quality)" in GIMP).
    #[default]
    Full,
    /// Colour at half resolution both ways ("4:2:0 (chroma quartered)").
    Quartered,
}
impl Subsampling {
    pub fn id(self) -> &'static str {
        match self {
            Self::Full => "444",
            Self::Quartered => "420",
        }
    }
}

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Annex K.1 quantisation tables, in natural (row-major) order.
const LUMA_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const CHROMA_Q: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// Annex K.3 Huffman tables: code counts by length 1..=16, then symbols.
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// A quantisation table scaled for `quality` (1..=100) as the IJG library scales it.
pub fn quant_table(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    std::array::from_fn(|i| ((u32::from(base[i]) * scale + 50) / 100).clamp(1, 255) as u16)
}

/// Canonical Huffman codes: `(code, length)` for each symbol byte.
fn huffman_codes(bits: &[u8; 16], vals: &[u8]) -> [(u16, u8); 256] {
    let mut table = [(0u16, 0u8); 256];
    let mut code = 0u16;
    let mut k = 0;
    for (len, count) in bits.iter().enumerate() {
        for _ in 0..*count {
            table[vals[k] as usize] = (code, len as u8 + 1);
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    table
}

/// `M[u][x] = 2^13 · ½·c(u)·cos((2x+1)uπ/16)`, rounded: `F = M·f·Mᵀ / 2^26`.
fn dct_matrix() -> [[i64; 8]; 8] {
    std::array::from_fn(|u| {
        std::array::from_fn(|x| {
            let c = if u == 0 {
                std::f64::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            let angle = ((2 * x + 1) * u) as f64 * fmath::PI / 16.0;
            fmath::round(8192.0 * 0.5 * c * fmath::cos(angle)) as i64
        })
    })
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}
impl Bits {
    fn put(&mut self, code: u32, len: u8) {
        for i in (0..u32::from(len)).rev() {
            self.acc = (self.acc << 1) | ((code >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                // A 0xFF in entropy-coded data is followed by a stuffed zero.
                if byte == 0xff {
                    self.out.push(0);
                }
                self.acc = 0;
                self.n = 0;
            }
        }
    }
    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad as u8);
        }
    }
}

/// Category (bit length) of a coefficient, and its low bits as T.81 codes them.
fn magnitude(v: i32) -> (u8, u32) {
    let a = v.unsigned_abs();
    let size = (32 - a.leading_zeros()) as u8;
    let bits = if v < 0 {
        (v - 1) as u32 & ((1u32 << size) - 1)
    } else {
        v as u32
    };
    (size, bits)
}

struct Component {
    quant: [u16; 64],
    dc: [(u16, u8); 256],
    ac: [(u16, u8); 256],
    pred: i32,
}

fn encode_block(bits: &mut Bits, block: &[i32; 64], c: &mut Component, m: &[[i64; 8]; 8]) {
    // Rows then columns of the fixed-point transform.
    let mut tmp = [[0i64; 8]; 8];
    for y in 0..8 {
        for u in 0..8 {
            tmp[y][u] = (0..8).map(|x| m[u][x] * i64::from(block[y * 8 + x])).sum();
        }
    }
    let mut coef = [0i32; 64];
    for v in 0..8 {
        for u in 0..8 {
            let sum: i64 = (0..8).map(|y| m[v][y] * tmp[y][u]).sum();
            // Quantise straight from the 2^26-scaled sum, rounding half away from zero.
            let q = i64::from(c.quant[v * 8 + u]) << 26;
            let r = if sum >= 0 {
                (sum + q / 2) / q
            } else {
                -((-sum + q / 2) / q)
            };
            coef[v * 8 + u] = r as i32;
        }
    }
    let dc = coef[0];
    let (size, low) = magnitude(dc - c.pred);
    c.pred = dc;
    let (code, len) = c.dc[size as usize];
    bits.put(u32::from(code), len);
    bits.put(low, size);
    let mut run = 0;
    for k in 1..64 {
        let v = coef[ZIGZAG[k]];
        if v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            let (code, len) = c.ac[0xf0];
            bits.put(u32::from(code), len);
            run -= 16;
        }
        let (size, low) = magnitude(v);
        let (code, len) = c.ac[(run << 4 | size as usize) & 0xff];
        bits.put(u32::from(code), len);
        bits.put(low, size);
        run = 0;
    }
    if run > 0 {
        let (code, len) = c.ac[0];
        bits.put(u32::from(code), len);
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

/// Opaque colour over white, as a JPEG must store it.
fn flat(p: Rgba) -> [i32; 3] {
    let a = u32::from(p[3]);
    std::array::from_fn(|c| (fmath::div255(u32::from(p[c]) * a + 255 * (255 - a))) as i32)
}

/// JFIF colour transform, fixed point with 16 fractional bits, every component already
/// level-shifted to be centred on zero as the transform wants (Y - 128, Cb, Cr).
fn ycc(p: [i32; 3]) -> [i32; 3] {
    let (r, g, b) = (i64::from(p[0]), i64::from(p[1]), i64::from(p[2]));
    let round = |v: i64| ((v + (1 << 15)) >> 16) as i32;
    [
        round(19595 * r + 38470 * g + 7471 * b) - 128,
        round(-11056 * r - 21712 * g + 32768 * b),
        round(32768 * r - 27440 * g - 5328 * b),
    ]
}

/// Encode `canvas` as a baseline JFIF file at `quality` (1..=100).
pub fn encode(canvas: &Canvas, quality: u8, subsampling: Subsampling) -> Vec<u8> {
    let (w, h) = (canvas.width() as i32, canvas.height() as i32);
    let luma_q = quant_table(&LUMA_Q, quality);
    let chroma_q = quant_table(&CHROMA_Q, quality);
    let mut out = vec![0xff, 0xd8];
    // JFIF 1.01, 72 dots per inch.
    segment(
        &mut out,
        0xe0,
        &[b'J', b'F', b'I', b'F', 0, 1, 1, 1, 0, 72, 0, 72, 0, 0],
    );
    for (id, table) in [(0u8, &luma_q), (1, &chroma_q)] {
        let mut body = vec![id];
        body.extend(ZIGZAG.iter().map(|i| table[*i] as u8));
        segment(&mut out, 0xdb, &body);
    }
    let luma_sampling = match subsampling {
        Subsampling::Full => 0x11,
        Subsampling::Quartered => 0x22,
    };
    let mut sof = vec![8];
    sof.extend_from_slice(&(h as u16).to_be_bytes());
    sof.extend_from_slice(&(w as u16).to_be_bytes());
    sof.extend_from_slice(&[3, 1, luma_sampling, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segment(&mut out, 0xc0, &sof);
    for (class_id, bits, vals) in [
        (0x00u8, &DC_LUMA_BITS, &DC_VALS[..]),
        (0x10, &AC_LUMA_BITS, &AC_LUMA_VALS[..]),
        (0x01, &DC_CHROMA_BITS, &DC_VALS[..]),
        (0x11, &AC_CHROMA_BITS, &AC_CHROMA_VALS[..]),
    ] {
        let mut body = vec![class_id];
        body.extend_from_slice(bits);
        body.extend_from_slice(vals);
        segment(&mut out, 0xc4, &body);
    }
    segment(&mut out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);

    let m = dct_matrix();
    let mut comps = [
        Component {
            quant: luma_q,
            dc: huffman_codes(&DC_LUMA_BITS, &DC_VALS),
            ac: huffman_codes(&AC_LUMA_BITS, &AC_LUMA_VALS),
            pred: 0,
        },
        Component {
            quant: chroma_q,
            dc: huffman_codes(&DC_CHROMA_BITS, &DC_VALS),
            ac: huffman_codes(&AC_CHROMA_BITS, &AC_CHROMA_VALS),
            pred: 0,
        },
        Component {
            quant: chroma_q,
            dc: huffman_codes(&DC_CHROMA_BITS, &DC_VALS),
            ac: huffman_codes(&AC_CHROMA_BITS, &AC_CHROMA_VALS),
            pred: 0,
        },
    ];
    // Colour-converted pixel, edges replicated past the image.
    let at = |x: i32, y: i32| ycc(flat(canvas.get(x.clamp(0, w - 1), y.clamp(0, h - 1))));
    let mut bits = Bits { out, acc: 0, n: 0 };
    let mcu = if subsampling == Subsampling::Full {
        8
    } else {
        16
    };
    for my in (0..h).step_by(mcu) {
        for mx in (0..w).step_by(mcu) {
            // Luma blocks, left to right then top to bottom within the MCU.
            for by in (0..mcu as i32).step_by(8) {
                for bx in (0..mcu as i32).step_by(8) {
                    let block: [i32; 64] = std::array::from_fn(|i| {
                        at(mx + bx + (i % 8) as i32, my + by + (i / 8) as i32)[0]
                    });
                    encode_block(&mut bits, &block, &mut comps[0], &m);
                }
            }
            for c in 1..3 {
                let block: [i32; 64] = std::array::from_fn(|i| {
                    let (x, y) = ((i % 8) as i32, (i / 8) as i32);
                    if mcu == 8 {
                        at(mx + x, my + y)[c]
                    } else {
                        // The mean of each 2x2, rounded half up.
                        let (x0, y0) = (mx + 2 * x, my + 2 * y);
                        let sum = at(x0, y0)[c]
                            + at(x0 + 1, y0)[c]
                            + at(x0, y0 + 1)[c]
                            + at(x0 + 1, y0 + 1)[c];
                        (sum + 2).div_euclid(4)
                    }
                });
                encode_block(&mut bits, &block, &mut comps[c], &m);
            }
        }
    }
    bits.flush();
    let mut out = bits.out;
    out.extend_from_slice(&[0xff, 0xd9]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image(w: u32, h: u32) -> Canvas {
        let mut c = Canvas::new(w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let r = (x * 255 / (w as i32 - 1)) as u8;
                let g = (y * 255 / (h as i32 - 1)) as u8;
                let b = (60 + ((x - 40) * (x - 40) + (y - 30) * (y - 30)) / 20).min(250) as u8;
                c.set(x, y, [r, g, b, 255]);
            }
        }
        // A hard-edged disc, where ringing would show.
        for y in 20..44 {
            for x in 30..54 {
                if (x - 42) * (x - 42) + (y - 32) * (y - 32) < 121 {
                    c.set(x, y, [250, 250, 20, 255]);
                }
            }
        }
        c
    }

    fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut d = jpeg_decoder::Decoder::new(bytes);
        let px = d.decode().unwrap();
        let info = d.info().unwrap();
        (u32::from(info.width), u32::from(info.height), px)
    }

    fn psnr(a: &Canvas, rgb: &[u8]) -> f64 {
        let mut se = 0u64;
        for (i, p) in a.pixels().chunks(4).enumerate() {
            for c in 0..3 {
                let d = i64::from(p[c]) - i64::from(rgb[i * 3 + c]);
                se += (d * d) as u64;
            }
        }
        let mse = se as f64 / (a.pixels().len() / 4 * 3) as f64;
        10.0 * fmath::ln(255.0 * 255.0 / mse) / fmath::ln(10.0)
    }

    #[test]
    fn tables_are_the_standard_ones() {
        assert_eq!(AC_LUMA_BITS.iter().map(|b| *b as usize).sum::<usize>(), 162);
        assert_eq!(
            AC_CHROMA_BITS.iter().map(|b| *b as usize).sum::<usize>(),
            162
        );
        assert_eq!(DC_LUMA_BITS.iter().map(|b| *b as usize).sum::<usize>(), 12);
        assert_eq!(
            DC_CHROMA_BITS.iter().map(|b| *b as usize).sum::<usize>(),
            12
        );
        // IJG quality 50 is the table itself, 100 is all ones.
        assert_eq!(quant_table(&LUMA_Q, 50), LUMA_Q);
        assert_eq!(quant_table(&LUMA_Q, 100), [1; 64]);
        assert_eq!(quant_table(&LUMA_Q, 75)[0], 8);
        let mut seen = [false; 64];
        for i in ZIGZAG {
            seen[i] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(magnitude(-3), (2, 0));
        assert_eq!(magnitude(5), (3, 5));
        assert_eq!(magnitude(0), (0, 0));
    }

    #[test]
    fn encodings_decode_close_to_the_original_and_are_byte_stable() {
        // Odd sizes exercise the replicated edges of partial MCUs.
        let img = test_image(83, 61);
        for (quality, sub, floor) in [
            (95, Subsampling::Full, 43.0),
            (90, Subsampling::Full, 38.5),
            (75, Subsampling::Quartered, 30.0),
            (50, Subsampling::Quartered, 29.0),
            (20, Subsampling::Full, 28.0),
        ] {
            let bytes = encode(&img, quality, sub);
            assert_eq!(&bytes[..2], &[0xff, 0xd8]);
            assert_eq!(&bytes[bytes.len() - 2..], &[0xff, 0xd9]);
            let (w, h, rgb) = decode(&bytes);
            assert_eq!((w, h), (83, 61));
            let p = psnr(&img, &rgb);
            assert!(p > floor, "quality {quality} {sub:?}: {p:.2} dB");
            assert_eq!(encode(&img, quality, sub), bytes, "deterministic");
        }
        let small = encode(&img, 30, Subsampling::Quartered).len();
        let large = encode(&img, 95, Subsampling::Full).len();
        assert!(small * 2 < large, "{small} vs {large}");
        // Pinned: any change to the transform, rounding or tables moves this.
        let bytes = encode(&img, 90, Subsampling::Quartered);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
        assert_eq!(
            format!("{h:016x}"),
            "e2ed878e2e736c2d",
            "{} bytes",
            bytes.len()
        );
    }

    #[test]
    fn transparency_flattens_over_white_and_flat_colour_is_exact() {
        let c = Canvas::filled(16, 16, [0, 0, 0, 0]);
        let (_, _, rgb) = decode(&encode(&c, 90, Subsampling::Full));
        assert!(rgb.iter().all(|v| *v == 255), "{:?}", &rgb[..6]);
        let c = Canvas::filled(24, 8, [200, 60, 30, 255]);
        let (_, _, rgb) = decode(&encode(&c, 100, Subsampling::Full));
        for p in rgb.chunks(3) {
            for (a, b) in p.iter().zip([200u8, 60, 30]) {
                assert!(a.abs_diff(b) <= 1, "{p:?}");
            }
        }
    }
}
